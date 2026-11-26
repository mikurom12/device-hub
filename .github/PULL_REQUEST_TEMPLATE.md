## Summary

-

## Device / hardware verification (driver changes only)

- Device and connection path:
- Read verified:
- Write/read-back verified:
- Not verified:

## Checks

- [ ] `cargo clippy --workspace --locked -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] Contract changes are additive only, and demo backend was updated
- [ ] UI uses capabilities, not device-name/VID/PID branching
- [ ] Third-party provenance/licenses were added to `THIRD_PARTY_NOTICES.md` when needed
