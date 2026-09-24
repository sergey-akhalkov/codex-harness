## 1. Close the executor tab

- [x] 1.1 Pass a tab-only close flag from Windows Terminal dispatch and make that host exit 0 after the session returns, without changing the receipt. Verify the tab-argument test includes the flag and a failing child under the flag returns 0 while the same child without the flag returns its own code.
- [x] 1.2 Update the host comment, executor help, agent-delegation guide and project decision so a failed tab closes and the receipt remains the inspection surface. Verify those texts no longer say a failure stays visible in the tab.

## 2. Keep a completed turn when the thread read is too large

- [x] 2.1 On a completed turn, recover the last delivered nonempty assistant message when the full-thread read exceeds the transport limit, and record an output defect that names the limit when no message was delivered. Other read failures still fail the host and terminate the child tree. Verify a hosted test with an oversized thread read completes from the delivered message and does not report a terminated child tree.

## 3. Check

- [x] 3.1 Run the executor tab and control tests that cover the close flag and the oversized read, and record the command and result.
