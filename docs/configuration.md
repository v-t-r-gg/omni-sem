# Configuration

Fresh installations are network-inert:

```toml
[embeddings]
enabled = false
provider = "none"
endpoint = ""
model = ""
batch_size = 16
request_timeout_seconds = 60
keep_alive = "5m"
truncate = false
dimensions = 0
```

Explicit Ollama example:

```toml
[embeddings]
enabled = true
provider = "ollama"
endpoint = "http://127.0.0.1:11434"
model = "nomic-embed-text:latest"
batch_size = 16
request_timeout_seconds = 60
keep_alive = "5m"
truncate = false
dimensions = 768
```

Batch size is 1–256, timeout 1–600 seconds, and dimensions are zero (provider output) or 8–65536. Only HTTP(S) endpoints with a host and without credentials, query, or fragment are accepted. Ollama requires an endpoint and model. Disabled configuration requires provider `none`, empty endpoint/model, and zero dimensions. Truncation must remain false. Unknown fields/providers are rejected.

Only `index` and provider resolution in `doctor` can use the endpoint. Init, root operations, status/server, changes, snapshots, lexical query, and lexical evaluation never contact it. Models are never detected, selected, downloaded, or pulled automatically.
