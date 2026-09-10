//! Owned stdio connection serving. Tool handlers must honor operation deadlines
//! and cancellation, and return only after reclaiming their owned process trees.
#![cfg(windows)]

use crate::{
    cancellable_pipe::{CancellablePipe, PipeIoError},
    mcp_protocol::{Decoder, Message, READ_CHUNK},
    mcp_session::{Operation, Session},
    process::{Cancellation, Deadline},
};
use serde_json::Value;
use std::{
    fs::File,
    io,
    os::windows::io::AsHandle,
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const POLL: Duration = Duration::from_millis(10);
const CLEANUP: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

enum Input {
    Message(Message),
    Eof,
}

fn failed(reason: &'static str) -> io::Error {
    io::Error::other(reason)
}

/// Duplicate only the current process's standard handles. The pipe layer checks
/// supported pipe handles; a console or disk redirection is not a stdio MCP peer.
pub fn standard_files() -> io::Result<(File, File)> {
    Ok((
        File::from(io::stdin().as_handle().try_clone_to_owned()?),
        File::from(io::stdout().as_handle().try_clone_to_owned()?),
    ))
}

fn send_input(
    sender: &SyncSender<Input>,
    mut input: Input,
    stop: &Cancellation,
    deadline: Deadline,
) -> io::Result<()> {
    loop {
        if stop.is_cancelled() {
            return Ok(());
        }
        if deadline.expired() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "MCP input deadline exceeded",
            ));
        }
        match sender.try_send(input) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Full(returned)) => input = returned,
            Err(TrySendError::Disconnected(_)) => return Ok(()),
        }
        thread::sleep(POLL.min(deadline.remaining()));
    }
}

fn input_worker(
    file: File,
    sender: SyncSender<Input>,
    stop: Cancellation,
    deadline: Deadline,
) -> io::Result<()> {
    let mut pipe = CancellablePipe::reader(file, stop.clone())?;
    let mut decoder = Decoder::default();
    let result = (|| {
        loop {
            let bytes = match pipe.read(READ_CHUNK, deadline, &stop) {
                Ok(bytes) => bytes,
                Err(PipeIoError::EndOfFile) => Vec::new(),
                Err(PipeIoError::Cancelled {
                    worker_joined: true,
                }) if stop.is_cancelled() => return Ok(false),
                Err(error) => return Err(error.into()),
            };
            if bytes.is_empty() {
                decoder.finish()?;
                return Ok(true);
            }
            decoder.push(&bytes)?;
            while let Some(message) = decoder.next_message()? {
                send_input(&sender, Input::Message(message), &stop, deadline)?;
                if stop.is_cancelled() {
                    return Ok(false);
                }
            }
        }
    })();
    let cleanup = pipe
        .close(Deadline::after(CLEANUP)?)
        .map_err(io::Error::from);
    match (result, cleanup) {
        (Ok(true), Ok(())) => send_input(&sender, Input::Eof, &stop, deadline),
        (Ok(false), Ok(())) => Ok(()),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(primary), Err(cleanup)) => Err(io::Error::new(
            primary.kind(),
            format!("{primary}; input cleanup failed: {cleanup}"),
        )),
    }
}

fn emit(
    pipe: &mut CancellablePipe,
    message: Message,
    stop: &Cancellation,
    connection_deadline: Deadline,
) -> io::Result<()> {
    let deadline = Deadline::after(WRITE_TIMEOUT.min(connection_deadline.remaining()))?;
    let bytes = message.encode()?;
    for chunk in bytes.chunks(READ_CHUNK) {
        pipe.write_all(chunk, deadline, stop)?;
    }
    Ok(())
}

fn join_bounded<T>(
    worker: JoinHandle<T>,
    deadline: Deadline,
    reason: &'static str,
) -> io::Result<T> {
    while !worker.is_finished() {
        if deadline.expired() {
            // No borrowed data or raw handles escape: the detached thread keeps
            // its owned values alive. This is an explicit cleanup failure and
            // the host must stop serving; it is never reported as clean shutdown.
            drop(worker);
            return Err(failed(reason));
        }
        thread::sleep(POLL.min(deadline.remaining()));
    }
    worker.join().map_err(|_| failed("MCP worker panicked"))
}

struct Task {
    id: Value,
    worker: JoinHandle<io::Result<Value>>,
}

/// Serve one connection. No global resources are discovered or started here.
/// `deadline` is an explicit maximum connection lifetime chosen by its owner.
/// The caller's cancellation token is read-only. A cleanup error means the host
/// must terminate this connection and cannot reuse the unfinished worker slot.
pub fn serve<F>(
    session: Session,
    input: File,
    output: File,
    cancellation: &Cancellation,
    deadline: Deadline,
    handler: F,
) -> io::Result<()>
where
    F: Fn(Operation) -> Value + Send + Sync + 'static,
{
    serve_fallible(
        session,
        input,
        output,
        cancellation,
        deadline,
        move |operation| Ok(handler(operation)),
    )
}

/// A backend infrastructure/cleanup error closes the connection, including
/// after cancellation or EOF. Only a successfully reclaimed operation may
/// produce a normal tool result and release the scheduler's active slot.
pub fn serve_fallible<F>(
    mut session: Session,
    input: File,
    output: File,
    cancellation: &Cancellation,
    deadline: Deadline,
    handler: F,
) -> io::Result<()>
where
    F: Fn(Operation) -> io::Result<Value> + Send + Sync + 'static,
{
    if cancellation.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "MCP serving cancelled before start",
        ));
    }
    if deadline.expired() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "MCP connection deadline expired",
        ));
    }
    let stop = Cancellation::default();
    let mut writer = CancellablePipe::writer(output, stop.clone())?;
    // At most two decoded input envelopes can wait behind the scheduler.
    let (sender, receiver): (SyncSender<Input>, Receiver<Input>) = mpsc::sync_channel(2);
    let reader = {
        let stop = stop.clone();
        thread::Builder::new()
            .name("mcp-input-dispatch".into())
            .spawn(move || input_worker(input, sender, stop, deadline))
    };
    let reader = match reader {
        Ok(reader) => reader,
        Err(error) => {
            stop.cancel();
            let cleanup = writer.close(Deadline::after(CLEANUP)?);
            return match cleanup {
                Ok(()) => Err(error),
                Err(cleanup) => Err(io::Error::new(
                    error.kind(),
                    format!("{error}; output cleanup failed: {cleanup}"),
                )),
            };
        }
    };
    let handler = Arc::new(handler);
    let mut task: Option<Task> = None;
    let result = (|| {
        loop {
            if cancellation.is_cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "MCP serving cancelled",
                ));
            }
            if deadline.expired() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "MCP connection deadline expired",
                ));
            }
            for reply in session.tick()? {
                emit(&mut writer, reply, &stop, deadline)?;
            }
            if task.as_ref().is_some_and(|task| task.worker.is_finished()) {
                let task = task.take().unwrap();
                let response = task
                    .worker
                    .join()
                    .map_err(|_| failed("MCP tool worker panicked; cleanup not confirmed"))??;
                if let Some(reply) = session.complete(&task.id, response)? {
                    emit(&mut writer, reply, &stop, deadline)?;
                }
            }
            if let Some(operation) = session.next_operation() {
                let id = operation.id.clone();
                let handler = handler.clone();
                let worker = thread::Builder::new()
                    .name("mcp-tool-dispatch".into())
                    .spawn(move || handler(operation))?;
                task = Some(Task { id, worker });
            }
            match receiver.recv_timeout(POLL) {
                Ok(Input::Message(message)) => {
                    if let Some(reply) = session.receive(message)? {
                        emit(&mut writer, reply, &stop, deadline)?;
                    }
                }
                Ok(Input::Eof) => return Ok(()),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(failed("MCP input ended without a clean EOF"));
                }
            }
        }
    })();
    session.close();
    stop.cancel();
    drop(receiver);
    let cleanup_deadline = Deadline::after(CLEANUP)?;
    let output_cleanup = writer.close(cleanup_deadline).map_err(io::Error::from);
    let input_cleanup = join_bounded(
        reader,
        cleanup_deadline,
        "MCP input dispatch remains detached after cleanup deadline",
    )
    .and_then(|result| result);
    let task_cleanup = task
        .map(|task| {
            join_bounded(
                task.worker,
                cleanup_deadline,
                "MCP tool dispatch remains detached after cleanup deadline",
            )
            .and_then(|result| result)
        })
        .transpose();
    let mut errors = Vec::new();
    if let Err(error) = output_cleanup {
        errors.push(format!("output: {error}"));
    }
    if let Err(error) = input_cleanup {
        errors.push(format!("input: {error}"));
    }
    if let Err(error) = task_cleanup {
        errors.push(format!("tool: {error}"));
    }
    if errors.is_empty() {
        return result;
    }
    let cleanup = errors.join("; ");
    match result {
        Ok(()) => Err(io::Error::other(cleanup)),
        Err(primary) => Err(io::Error::new(
            primary.kind(),
            format!("{primary}; {cleanup}"),
        )),
    }
}
