// Static contract for release safety. This test deliberately avoids a YAML
// dependency so it can run before any package installation or publication.
"use strict";

import "./release_contract.workflow.mjs";
import "./release_contract.artifacts.mjs";

console.log("release_contract.test: OK");
