// Static contract for release safety. This test deliberately avoids a YAML
// dependency so it can run before any package installation or publication.
"use strict";

import "./release_contract.workflow.mjs";
import "./release_contract.globals.mjs";
import "./release_contract.authorization.mjs";
import "./release_contract.artifacts.mjs";
import "./release_contract.staging.mjs";
import "./release_order.test.mjs";
import "./resource_boundary.test.mjs";
await import("./npm_publication.test.mjs");
await import("./release_process.test.mjs");
await import("./npm_publication_state.test.mjs");
await import("./release_aggregate.test.mjs");
await import("./npm_publish_cli.test.mjs");

await import("./release_receipt.test.mjs");
await import("./release_receipt_aggregation.test.mjs");
await import("./release_receipt_cli.test.mjs");
await import("./channel_promotion.test.mjs");
await import("./npm_publisher_auth.test.mjs");
await import("./npm_publisher_preflight.test.mjs");
await import("./github_staging.test.mjs");
await import("./npm_channel_promotion.test.mjs");
await import("./github_channel_promotion.test.mjs");
await import("./native_channel_consumer.test.mjs");
await import("./native_receipt.test.mjs");
await import("./crate_publication.test.mjs");

console.log("release_contract.test: OK");
