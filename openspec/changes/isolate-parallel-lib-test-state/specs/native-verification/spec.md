## ADDED Requirements

### Requirement: Parallel lib tests share no cross-test state

harness-core lib tests SHALL NOT depend on ambient state mutated by other concurrently running tests; the full parallel lib suite SHALL pass repeatedly without serialization or narrowed filters.

#### Scenario: Repeated full parallel runs are deterministic
- **WHEN** the full harness-core lib suite runs in parallel three consecutive times
- **THEN** every run passes without `-j 1` and without reducing test selection
