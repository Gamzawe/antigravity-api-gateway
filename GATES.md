# Gates: Antigravity Gateway Upgrade Implementation (Phase 1 & Phase 2)

OWNS: src/**, tests/**, deploy/**, Dockerfile, docker-compose.yml, package.json

- [x] G1: Milestone 1 unit tests pass (WP-01 through WP-05) proving stream replay elimination, tool finish reason 'tool_calls', framing SSE, canonical model scoping, and account eligibility.
  CHECK: node --test tests/unit/m1-*.test.mjs
  EXPECT: pass 6
  EVIDENCE: automatic-evidence=v1; definition-sha256=71478237a06e503aed0f9538ea8d1e72cedbe43a072685591b896617dfd87325; exit=0; EXPECT=matched; output-sha256=07d6a5e03170aae720bbb9c4cc949232db6c77bee150b70626393f273ebf046f; output-bytes=1174; shell=C:\WINDOWS\system32\cmd.exe; cwd=D:\Side-Projects\antigravity-api-gateway; path=928817dd1325/62 entries

- [x] G2: Milestone 2 unit tests pass (WP-06 through WP-08) proving single-flight auth refresh, quota reset parsing, and serialized account persistence with dynamic reload.
  CHECK: node --test tests/unit/m2-*.test.mjs
  EXPECT: pass 3
  EVIDENCE: automatic-evidence=v1; definition-sha256=b426bbb3a7796c474da18b23367e2c3a2d7d10c4166fad685fec7f2a609c951f; exit=0; EXPECT=matched; output-sha256=fe08c20dd09bafa5994ad08b0f180281db27f50d0d7fa522c22e664065e7274e; output-bytes=703; shell=C:\WINDOWS\system32\cmd.exe; cwd=D:\Side-Projects\antigravity-api-gateway; path=928817dd1325/62 entries

- [x] G3: Milestone 3 unit tests pass (WP-09 through WP-12) proving atomic attempt leases, bounded fair admission queue, backpressure handling, telemetry token accounting, and capability-aware model mapping.
  CHECK: node --test tests/unit/m3-*.test.mjs
  EXPECT: pass 3
  EVIDENCE: automatic-evidence=v1; definition-sha256=02fe0f1aa000dd1c759584643bda172e981eaed79d38709da6c6af37ee03a6a4; exit=0; EXPECT=matched; output-sha256=c3d241491b32e744c29bc2f90535e10aabbd26abd5bf888d00b9d0ab9c14d854; output-bytes=411; shell=C:\WINDOWS\system32\cmd.exe; cwd=D:\Side-Projects\antigravity-api-gateway; path=928817dd1325/62 entries

- [x] G4: Zero regressions across existing console and security test suites.
  CHECK: npm run test:console
  EXPECT: pass 17
  EVIDENCE: automatic-evidence=v1; definition-sha256=435102375ec566c894bb4bc6a429a834a7c4da5e13915e960ccecd8b9a0c9e9e; exit=0; EXPECT=matched; output-sha256=6561453cbb666b86491ea3d5bdb5a63039c627ea0bf087637e651e1c56898e34; output-bytes=2025; shell=C:\WINDOWS\system32\cmd.exe; cwd=D:\Side-Projects\antigravity-api-gateway; path=928817dd1325/62 entries
