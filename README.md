# xmip-core-resilience-retry

Retry guard: a retryable failure below the attempt limit is tried again after a delay, flat or backing off. A technology of [xmip-core-resilience](https://github.com/IlleNilsson/xmip-core-resilience).

It is what a Send Port's `retry = { attempts, backoff }` is: `xmip-service` links it and hands it to the node it starts, which builds one for each Send Port stating `retry`, `attempts` tries after the first and `backoff` between them, flat, and asks it after each try that did not send (ADR-0048, amendment 2026-11-14).

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
