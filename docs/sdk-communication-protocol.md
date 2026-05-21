# SDK and Harness Communication Protocol

This document details the technical communication architecture and binary/structured network protocol used for synchronization between the **Google Antigravity SDK** and the **`localharness`** execution engine.

---

## 1. Connection Architecture

The communication is structured as a two-phase connection sequence:

```
+-------------+                     +---------------+
|  Client SDK |                     | localharness  |
+-------------+                     +---------------+
       |                                    |
       | 1. Spawn Process & Pipe Stdin      |
       |----------------------------------->|
       |    (InputConfig Protobuf payload)  |
       |                                    |
       | 2. Read Stdout                     |
       |<-----------------------------------|
       |    (OutputConfig Protobuf payload) |
       |                                    |
       | 3. Connect WebSocket (X-goog-api-key)|
       |----------------------------------->|
       |    ws://localhost:<port>/          |
       |                                    |
       | 4. Send InitializeConversation     |
       |----------------------------------->|
       |    (JSON Config: system, tools...) |
       |                                    |
       | <-------- Bi-directional --------> |
       |          (JSON Event Stream)       |
```

---

## 2. Phase 1: Startup Handshake (Protobuf over Stdin/Stdout)

To prevent platform issues and dependency chains on `protoc`, the SDK uses a zero-dependency binary encoder/decoder to parse configuration values directly over the process's standard input and output streams.

### Input Configuration (`InputConfig`)
Sent from the **SDK to `localharness`** standard input.
* **Fields**:
  - `storage_directory` (Field Number `1`, Wire Type `2` - Length-delimited string): The designated workspace and config preservation path.
* **Serialized Form**:
  - Encoded using standard Protobuf varint length tags (e.g. tag `0x0A` representing field `1` of wire-type `2`).

### Output Configuration (`OutputConfig`)
Sent from **`localharness` to the SDK** standard output.
* **Fields**:
  - `port` (Field Number `1`, Wire Type `0` - Varint i32): Ephemeral local TCP port bound by the harness WebSocket server.
  - `api_key` (Field Number `2`, Wire Type `2` - Length-delimited string): A generated session authorization token.

---

## 3. Phase 2: WebSocket Initialization

Once the SDK decodes the TCP port and `api_key`, it establishes a local WebSocket connection:

- **Endpoint**: `ws://localhost:<port>/`
- **Authentication Header**:
  ```http
  x-goog-api-key: <api_key_from_output_config>
  ```
- **First Message**: The SDK immediately transmits an `InitializeConversationEvent` JSON payload:
  ```json
  {
    "config": {
      "tools": [
        {
          "name": "lookup_fruit_sku",
          "description": "Looks up the SKU for a given fruit.",
          "parametersJsonSchema": "{\"type\":\"object\",...}"
        }
      ],
      "systemInstructions": {
        "custom": {
          "part": [{"text": "You keep track of fruit inventory..."}]
        }
      },
      "geminiConfig": {
        "model": "gemini-2.5-flash",
        "apiKey": "YOUR_API_KEY",
        "thinkingConfig": {
          "thinkingBudget": 1024,
          "thinkingLevel": "high"
        }
      },
      "workspaces": [
        {
          "filesystemWorkspace": {
            "directory": "/home/user/workspace/antigravity-sdk"
          }
        }
      ],
      "appDataDir": "/home/user/.gemini/antigravity"
    }
  }
  ```

---

## 4. Phase 3: Bi-Directional Event Stream Protocol

During the conversation session, the SDK and `localharness` communicate via two JSON message schemas: **`OutputEvent`** (Harness $\rightarrow$ SDK) and **`InputEvent`** (SDK $\rightarrow$ Harness).

### A. Harness $\rightarrow$ SDK (`OutputEvent`)
Each WebSocket frame sent by `localharness` contains sequence metadata along with one of three primary payloads:

```rust
pub struct OutputEvent {
    pub seq_num: Option<u64>,
    pub timestamp_micros: Option<u64>,
    pub step_update: Option<StepUpdate>,
    pub trajectory_state_update: Option<TrajectoryStateUpdate>,
    pub tool_call: Option<ToolCall>,
    pub usage_metadata: Option<UsageMetadata>,
}
```

1. **`step_update`**: Real-time status updates of an agent execution block. Contains token streaming data:
   - `text_delta` / `thinking_delta`: Incremental textual output and chain-of-thought reasoning streamed directly from the model.
   - `state` / `source` / `target`: Metadata tracking who is executing (`System`, `User`, `Model`) and the current state (`Active`, `Done`, `WaitingForUser`, `Error`).
2. **`trajectory_state_update`**: Updates the overall status of the agent process (`Running` vs `Idle`). The SDK uses this to synchronize waiting flags (`wait_for_idle`).
3. **`tool_call`**: Requests execution of a client-side custom tool.
   - `id`: Unique tool invocation sequence ID.
   - `name`: Matches the registered custom tool's identifier.
   - `arguments_json`: Raw JSON arguments compiled by the LLM.

---

### B. SDK $\rightarrow$ Harness (`InputEvent`)
Whenever a user interacts with the agent, or when the SDK completes a delegated callback, it transmits an `InputEvent`:

```rust
pub struct InputEvent {
    pub user_input: Option<String>,
    pub complex_user_input: Option<UserInput>,
    pub tool_confirmation: Option<ToolConfirmation>,
    pub tool_response: Option<ToolResponse>,
    pub question_response: Option<UserQuestionsResponse>,
    pub halt_request: Option<bool>,
    pub automated_trigger: Option<String>,
}
```

1. **`user_input` / `complex_user_input`**: Initiates a new prompt or streams user responses/multimodal media attachments into the active execution context.
2. **`tool_confirmation`**: Replies to safety policy queries.
   - `trajectory_id` & `step_index`: Identifies which execution block is blocked.
   - `accepted` (`true` / `false`): Tells the harness whether the safety filter (e.g. running a shell command) was approved by the SDK/user policies.
3. **`tool_response`**: Returns the computed outcome of a custom tool callback.
   - `id`: Corresponds to the incoming tool request's ID.
   - `response_json`: JSON-encoded payload produced by the SDK's execution of the closure.

---

## 5. Lifecycle Safety and Resource Cleanup

To prevent orphaned background subprocesses:
* The standard input pipe of `localharness` (`ChildStdin`) is retained in a thread-safe `Mutex` inside the `Connection` context.
* If `ChildStdin` is dropped or closed (e.g. when `Agent::stop()` is called), `localharness` immediately triggers a shutdown sequence:
  ```
  harness stderr: Stdin closed, cleaning up...
  ```
* This ensures that terminating the SDK cleanly terminates the harness process, releasing ephemeral WebSocket listeners and local locks.
