# Patched mobile dependencies

`image-size@1.2.1.patch` prevents malformed ICNS and JXL container lengths from
leaving the parser in a non-advancing loop. The installed 1.2.1 package already
contains the equivalent zero-length guard in its shared HEIF/JXL box scanner.

pnpm's advisory database marks every `image-size <= 2.0.2` release as affected
by GHSA-5p2g-fcmc-qvqq and GHSA-w3rx-r6r6-pgpr and does not list a fixed npm
release. Those two advisory IDs are ignored only after this patch is applied;
`src/security/image-size-security.test.ts` executes malformed ICNS, JXL, and
HEIF inputs in child processes with a hard timeout so a missing regression fix
fails safely instead of hanging the test runner.
