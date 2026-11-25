# Estimate texture payload by mip level

Run the budget report on an RDS5 texture store:

```bash
forge9 texture store.ttpf --budget > budget.json
```

The command reads the store index and estimates the number of tightly packed bytes
needed by its declared mip levels. It includes known BC1–BC7, RGBA8, and BGRA8
textures in tile modes 0–3, including their array slices. The [BC block
sizes](https://learn.microsoft.com/en-us/windows/win32/direct3d11/texture-block-compression-in-direct3d-11)
apply even when a level occupies less than one 4×4 block. Entries outside this
coverage are counted in `skipped_textures`.

In the `forge9-texture-budget-v1` report, each four-element array corresponds to
omitting 0, 1, 2, or 3 of the largest mips. Each scenario retains the smallest
declared mip. `saved_payload_bytes` compares each scenario with the original
chain. `by_format` includes the texture count and payload for each DXGI format.

Each texture is counted once per index entry. Resource aliasing, deduplication
across stores, tile and row padding, driver allocations, and concurrent residency
are outside this estimate. Use it to rank candidates for a controlled runtime
test. Measure peak allocation and frame time at the same scene and internal
render resolution with and without a mip policy change.
