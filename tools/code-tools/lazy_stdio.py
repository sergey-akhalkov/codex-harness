"""A serialized MCP subprocess owned by one task, retired between idle requests."""
from __future__ import annotations

import asyncio
from contextlib import asynccontextmanager, suppress
import os
from pathlib import Path
import subprocess
import sys
import time

import anyio
from mcp import ClientSession
from mcp.client.stdio import get_default_environment, stdio_client
from mcp.shared.message import SessionMessage
from mcp.types import JSONRPCMessage

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from process_ownership import JobGuard

MAX_MESSAGE = 16 * 1024 * 1024


class RequestRejected(ValueError):
    """A preflight refusal: no native operation ran and its session is usable."""


@asynccontextmanager
async def owned_stdio(parameters):
    """MCP streams with atomic Windows ownership and unconditional tree close.

    The native SDK's Windows transport assigns its Job after spawn and retains
    grandchildren after a graceful parent exit. The kit's JobGuard provides the
    required creation and shutdown contracts; MCP message/session types remain
    the SDK's. Other platforms retain the native SDK transport behavior.
    """
    if os.name != 'nt':
        async with stdio_client(parameters) as streams:
            yield streams
        return
    guard = JobGuard()
    process = None
    incoming, receive = anyio.create_memory_object_stream(0)
    send, outgoing = anyio.create_memory_object_stream(0)
    try:
        process = guard.popen([parameters.command, *parameters.args],
                    env={**get_default_environment(), **(parameters.env or {})}, cwd=parameters.cwd,
                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=sys.stderr,
                    creationflags=subprocess.CREATE_NO_WINDOW)

        async def read():
            async with incoming:
                while True:
                    raw = await anyio.to_thread.run_sync(process.stdout.readline, MAX_MESSAGE + 1, abandon_on_cancel=True)
                    if not raw:
                        break
                    if len(raw) > MAX_MESSAGE:
                        await incoming.send(ValueError('Native MCP response exceeds 16 MiB'))
                        break
                    try:
                        message = JSONRPCMessage.model_validate_json(raw.decode(parameters.encoding, parameters.encoding_error_handler))
                    except Exception as error:
                        await incoming.send(error)
                        break
                    await incoming.send(SessionMessage(message))

        def write_raw(raw):
            process.stdin.write(raw)
            process.stdin.flush()

        async def write():
            async with outgoing:
                async for message in outgoing:
                    raw = (message.message.model_dump_json(by_alias=True, exclude_none=True) + '\n').encode(
                        parameters.encoding, parameters.encoding_error_handler)
                    if len(raw) > MAX_MESSAGE:
                        raise ValueError('Native MCP request exceeds 16 MiB')
                    await anyio.to_thread.run_sync(write_raw, raw, abandon_on_cancel=True)

        async with anyio.create_task_group() as group:
            group.start_soon(read)
            group.start_soon(write)
            try:
                yield receive, send
            finally:
                # Kill even when the immediate parent already exited. Do this
                # before cancelling pipe readers/writers so kernel I/O unblocks.
                with anyio.CancelScope(shield=True):
                    await anyio.to_thread.run_sync(guard.close)
                group.cancel_scope.cancel()
    finally:
        with anyio.CancelScope(shield=True):
            await anyio.to_thread.run_sync(guard.close)
            if process is not None:
                await anyio.to_thread.run_sync(lambda: process.wait(timeout=5))
                for stream in (process.stdin, process.stdout):
                    if stream:
                        stream.close()
            for stream in (incoming, receive, send, outgoing):
                await stream.aclose()


class LazyStdio:
    def __init__(self, parameters, *, idle_seconds=300, timeout=60, on_idle=None, lease_for=None, before_request=None, after_request=None):
        self.parameters = parameters
        self.idle_seconds = idle_seconds
        self.timeout = timeout
        self.on_idle = on_idle
        self.lease_for = lease_for
        self.before_request = before_request
        self.after_request = after_request
        self.queue = asyncio.Queue(maxsize=64)
        self.task = None

    async def __aenter__(self):
        self.task = asyncio.create_task(self._serve())
        return self

    async def __aexit__(self, *_):
        self.task.cancel()
        with suppress(asyncio.CancelledError):
            await self.task

    async def request(self, method, *arguments):
        if self.task is None or self.task.done():
            raise RuntimeError('Owned MCP worker is unavailable')
        future = asyncio.get_running_loop().create_future()
        try:
            self.queue.put_nowait((method, arguments, future, time.monotonic() + self.timeout))
        except asyncio.QueueFull:
            raise RuntimeError('Owned MCP request queue is full; no operation was started') from None
        return await asyncio.wait_for(future, self.timeout)

    async def initialize(self):
        return await self.request('initialize')

    async def list_tools(self):
        return await self.request('list_tools')

    async def call_tool(self, name, arguments):
        return await self.request('call_tool', name, arguments)

    async def list_resources(self):
        return await self.request('list_resources')

    async def read_resource(self, uri):
        return await self.request('read_resource', uri)

    async def _serve(self):
        pending = None
        lease = None
        try:
            while True:
                pending = await self.queue.get()
                if pending[2].cancelled() or pending[3] <= time.monotonic():
                    if not pending[2].done():
                        pending[2].set_exception(TimeoutError('Queued MCP request expired before startup'))
                    pending = None
                    continue
                try:
                    async with owned_stdio(self.parameters) as streams, ClientSession(*streams) as remote:
                        info = await asyncio.wait_for(remote.initialize(), min(20, self.timeout))
                        while True:
                            method, arguments, future, deadline = pending
                            if not future.cancelled():
                                remaining = deadline - time.monotonic()
                                if remaining <= 0:
                                    raise TimeoutError('Queued MCP request expired before execution')
                                selected = self.lease_for(method, arguments) if self.lease_for else None
                                if selected:
                                    acquiring = asyncio.create_task(asyncio.to_thread(selected.__enter__))
                                    try:
                                        await asyncio.shield(acquiring)
                                    except asyncio.CancelledError:
                                        # A blocking lock acquisition can finish
                                        # after cancellation; retain its outcome.
                                        await acquiring
                                        lease = selected
                                        raise
                                    lease = selected
                                if time.monotonic() >= deadline:
                                    raise TimeoutError('MCP request expired while waiting for admission')
                                try:
                                    if self.before_request:
                                        updated = await self.before_request(method, arguments)
                                        if updated is not None:
                                            arguments = updated
                                except RequestRejected as error:
                                    # Invalid client input does not invalidate
                                    # the owned browser or current native state.
                                    if not future.done():
                                        future.set_exception(error)
                                else:
                                    # Cancellation or timeout keeps the lease until
                                    # owned_stdio and on_idle reclaim execution.
                                    result = info if method == 'initialize' else await asyncio.wait_for(
                                        getattr(remote, method)(*arguments), max(0.01, deadline - time.monotonic()))
                                    if self.after_request:
                                        result = await self.after_request(method, arguments, result)
                                    if not future.done():
                                        future.set_result(result)
                                if lease:
                                    lease.__exit__(None, None, None)
                                    lease = None
                            pending = None
                            try:
                                pending = await asyncio.wait_for(self.queue.get(), self.idle_seconds)
                            except asyncio.TimeoutError:
                                break
                except Exception as error:
                    if pending is not None and not pending[2].done():
                        pending[2].set_exception(error)
                    pending = None
                finally:
                    try:
                        if self.on_idle:
                            await asyncio.to_thread(self.on_idle)
                    finally:
                        if lease:
                            lease.__exit__(None, None, None)
                            lease = None
        finally:
            if pending is not None and not pending[2].done():
                pending[2].set_exception(RuntimeError('Owned MCP worker stopped'))
            while not self.queue.empty():
                _, _, future, _ = self.queue.get_nowait()
                if not future.done():
                    future.set_exception(RuntimeError('Owned MCP worker stopped'))
