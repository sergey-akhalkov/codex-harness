## ADDED Requirements

### Requirement: Executor tabs close when the host ends

When dispatch opens an executor conversation as a Windows Terminal tab, that tab SHALL close after the host process ends, including when the recorded run failed, was interrupted, or ended as an output defect. Closing SHALL NOT send a terminal command, SHALL NOT close the lead's window, a sibling tab, or another conversation, and SHALL NOT change the receipt's recorded state or exit code. The receipt, detail file and control log remain the inspection surface. An owned console that is not a terminal tab SHALL keep returning the run's own exit code. A process killed before the host can exit is outside this close path; stop continues to close its tab by ending that run's host.

#### Scenario: A failed executor tab closes

- **WHEN** an executor tab's host records a failed run and the host process ends
- **THEN** that tab closes, the receipt still records the failure and its cause, and no other conversation's tab closes

#### Scenario: A successful executor tab still closes

- **WHEN** an executor tab's host records a completed run and the host process ends
- **THEN** that tab closes and the receipt records the completed outcome

#### Scenario: An owned console keeps the run exit code

- **WHEN** the same failed run is hosted in an owned console instead of a terminal tab
- **THEN** the console process returns the run's own non-zero exit code and the receipt records the failure

### Requirement: An oversized final-message read does not fail a completed turn

After a turn has completed, the host SHALL read the final assistant message without treating a transport size limit on a full-thread read as a failed run. When that read exceeds the transport limit and the turn already delivered a nonempty assistant message, the host SHALL record that message and complete the run. When no such message was delivered, the host SHALL record an output defect that names the size limit, and SHALL NOT report success. In either case the host SHALL NOT terminate the owned child tree as a crash and SHALL NOT leave the failure unrecorded. A final-message read that fails for a reason other than the transport size limit SHALL still fail the host and terminate the owned child tree.

#### Scenario: The full thread exceeds the transport limit

- **WHEN** a turn completes, its assistant message was already delivered, and reading the whole thread exceeds the transport limit
- **THEN** the run is recorded completed with that message, the child tree is not terminated as a crash, and the receipt does not say the final message could not be read

#### Scenario: No delivered message and the thread read is too large

- **WHEN** a turn completes without a delivered nonempty assistant message and the full-thread read exceeds the transport limit
- **THEN** the receipt records an output defect that names the size limit, and the run is not reported as a successful completion

#### Scenario: Another final-message read failure still fails the host

- **WHEN** a completed turn's final-message read fails for a reason other than the transport size limit
- **THEN** the host records the failure, terminates the owned child tree, and does not report a successful run
