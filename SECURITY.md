# Security

If you believe you have found a security vulnerability in Hardgate, please
report it privately to [contact@techbytefrontier.com](mailto:contact@techbytefrontier.com).

Include a short description, the affected version or commit, the steps needed
to reproduce the issue, and any relevant environment details. Please avoid
sharing secrets or other sensitive data unless it is necessary to demonstrate
the issue.

Please do not disclose a suspected vulnerability publicly until maintainers
have had an opportunity to assess it.

For general bugs and questions, use the project's [issue
tracker](https://github.com/Tech-Byte-Frontier/hardgate/issues).

## Project commands

Ordinary checks and formatting execute trusted project commands natively on
macOS and Linux. Disposable check copies and input verification detect persistent
source changes; they do not sandbox malicious commands or prevent access to other
host files. Set `[orchestration] require_isolation = true` when CPU/memory limits
and protected Linux check workspaces are required. Evidence producers always
require isolation. Missing required protection fails the command; it is never
silently replaced with native execution.
