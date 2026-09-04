## Summary

Describe the behavior change and the reason for it. Keep the scope focused.

## Validation

List the exact commands and toolchain versions used. Include focused checks and,
when applicable, the complete CI/self-gate result or why a complete check was
not run.

```text
Commands:
Results:
```

## Reproduction or fixtures

For a bug or behavior change, link the smallest deterministic fixture or test
and state the expected and actual result.

## Checklist

- [ ] The change preserves the pinned toolchains and package identity checks.
- [ ] The relevant checks are listed above; mutation and builds were serialized.
- [ ] No threshold, exclusion, suppression, required evidence, or signing policy was weakened.
- [ ] Logs and examples contain no credentials or unrelated private data.
