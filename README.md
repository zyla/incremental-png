# incremental-png

Incremental PNG decoder.

Why another PNG decoder? I needed one which works under the following constraints:

- `no_std`
- `no_alloc`, all memory buffers are passed explicitly
- The input file doesn't necessarily fit in RAM, so process incrementally
