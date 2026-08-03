# In-process embedding provider investigation

Decision: **defer**.

| Approach | License and health | Runtime/packaging | Identity contract |
|---|---|---|---|
| ONNX Runtime (`ort`) | MIT wrapper; active | large native runtime, per-platform binaries, CPU/GPU variance | hash model, tokenizer/config, runtime/provider |
| FastEmbed-compatible Rust | commonly Apache-2.0/MIT; evolving | convenient downloads/cache but ONNX/tokenizer graph | pin artifact and preprocessing digests |
| Candle | Apache-2.0/MIT; active | Rust core, optional CUDA/Metal, model/tokenizer packaging | hash weights, tokenizer, pooling, precision/backend |

ONNX/FastEmbed is the likeliest next prototype, but offline distribution, native cross-platform support, startup/binary size, API stability, and preprocessing identity need measurements. No speculative dependency is added.
