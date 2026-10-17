# Dependent batch steps

Each batch step may bind operation parameters to an earlier completed step's
response. `step` is zero-based; `path` is an RFC 6901 JSON Pointer into the
corresponding entry in `results`. Values retain their JSON types.

For example, after a step that returns `result.handles`, group those entities:

```json
{"op":"group_create","name":"walls","bindings":{"handles":{"step":0,"path":"/result/handles"}}}
```

Use the actual result shape of the source operation. State and changes are stored
separately from step results and cannot be referenced through these bindings.
Only declared operation parameters may be bound. A parameter cannot have both
a literal value and a binding. Forward references and references to the current
step are rejected before dispatch, as are malformed bindings and invalid static
parameters. Resolved parameters receive normal operation validation before use.

An unavailable path, unsuccessful source, or invalid resolved type fails the
dependent step and stops the batch. Batches are not transactions: earlier changes
remain applied. Retrying the same request and request_id returns the retained
failure without replaying earlier steps. Keep the same request_id when resuming
a running batch. Use response_detail `summary` to reduce output, and read
`batch_result` to retrieve retained details without executing anything.
