## ADDED Requirements

### Requirement: SSE connection to Anthropic Messages API
The streaming client SHALL send a POST request to `/v1/messages` with `stream: true` in the body, including headers `x-api-key`, `anthropic-version: 2023-06-01`, and `content-type: application/json`. The client SHALL use `eventsource-stream` to parse the SSE byte stream from reqwest's `bytes_stream()`.

#### Scenario: Successful streaming connection
- **WHEN** the client sends a valid streaming request
- **THEN** the client receives SSE events starting with `message_start` and ending with `message_stop`

#### Scenario: API key missing
- **WHEN** the `ANTHROPIC_API_KEY` environment variable is not set
- **THEN** the client SHALL return an error before making any HTTP request

### Requirement: StreamEvent parsing
The client SHALL deserialize each SSE event's `data` field into a `StreamEvent` enum with variants: `MessageStart`, `ContentBlockStart`, `ContentBlockDelta`, `ContentBlockStop`, `MessageDelta`, `MessageStop`, `Ping`, `Error`. The `type` field in the JSON SHALL be used as the serde tag discriminator.

#### Scenario: Text streaming
- **WHEN** a `content_block_delta` event with `delta.type == "text_delta"` arrives
- **THEN** the `StreamEvent::ContentBlockDelta` variant SHALL contain a `Delta::TextDelta { text: String }`

#### Scenario: Tool use streaming
- **WHEN** a `content_block_delta` event with `delta.type == "input_json_delta"` arrives
- **THEN** the `StreamEvent::ContentBlockDelta` variant SHALL contain a `Delta::InputJsonDelta { partial_json: String }`

#### Scenario: Ping events
- **WHEN** a `ping` event arrives
- **THEN** the client SHALL ignore it (no processing, no error)

### Requirement: Partial JSON accumulation for tool inputs
The client SHALL maintain a `StreamAccumulator` that maps content block index to accumulated partial JSON string. The accumulator SHALL concatenate all `partial_json` values for a given block index. The accumulated JSON SHALL only be parsed when `content_block_stop` is received for that index.

#### Scenario: Multi-chunk tool input
- **WHEN** three `input_json_delta` events arrive for block index 1 with values `{"loc`, `ation":`, `"SF"}`
- **THEN** at `content_block_stop` for index 1, the parsed JSON SHALL be `{"location": "SF"}`

#### Scenario: Empty first delta
- **WHEN** the first `input_json_delta` for a block has `partial_json: ""`
- **THEN** the accumulator SHALL handle it correctly (empty string concatenation)

### Requirement: Retry with exponential backoff
The client SHALL retry failed requests up to 3 times with exponential backoff (1s, 2s, 4s). Retryable errors include HTTP 429 (rate limit), 500, 502, 503, 529 (overloaded). Non-retryable errors (400, 401, 403, 404) SHALL fail immediately.

#### Scenario: Rate limited
- **WHEN** the API returns HTTP 429
- **THEN** the client SHALL wait and retry up to 3 times

#### Scenario: Invalid API key
- **WHEN** the API returns HTTP 401
- **THEN** the client SHALL fail immediately without retry
