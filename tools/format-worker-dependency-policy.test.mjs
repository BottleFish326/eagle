import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const fixedVcpkgBaseline = "33e5269bbfc24fb252bc48a3e624c8193afdccce";

test("pins every bundled worker to decoder-only libheif 1.23.1 dependencies", async () => {
  const manifest = JSON.parse(
    await readFile("support/vcpkg/vcpkg.json", "utf8"),
  );
  assert.deepEqual(Object.keys(manifest).sort(), [
    "$schema",
    "builtin-baseline",
    "dependencies",
    "name",
    "version-string",
  ]);
  assert.equal(manifest["builtin-baseline"], fixedVcpkgBaseline);
  assert.deepEqual(manifest.dependencies, [
    {
      name: "libheif",
      "default-features": false,
      features: ["aom"],
    },
  ]);
  assert.doesNotMatch(JSON.stringify(manifest), /x26[45]/iu);

  const cargoManifest = await readFile(
    "crates/format-worker/Cargo.toml",
    "utf8",
  );
  assert.match(cargoManifest, /libheif-rs = \{ version = "=3\.0\.0"/u);
  const cargoLock = await readFile("Cargo.lock", "utf8");
  assert.match(cargoLock, /name = "libheif-rs"\nversion = "3\.0\.0"/u);
  assert.match(
    cargoLock,
    /name = "libheif-sys"\nversion = "5\.3\.1\+1\.23\.1"/u,
  );

  const tauriConfig = JSON.parse(
    await readFile("apps/desktop/src-tauri/tauri.conf.json", "utf8"),
  );
  assert.deepEqual(tauriConfig.bundle.resources, { "resources/": "" });
  const desktopRuntime = await readFile(
    "apps/desktop/src-tauri/src/lib.rs",
    "utf8",
  );
  assert.match(
    desktopRuntime,
    /resource_dir\(\)\?\.join\("format-workers\/libheif"\)/u,
  );

  const workflow = await readFile(".github/workflows/ci.yml", "utf8");
  assert.match(workflow, new RegExp(`ref: ${fixedVcpkgBaseline}`, "u"));
  assert.match(workflow, /"libheif\[aom\]:x64-windows-static-md"/u);
  assert.doesNotMatch(workflow, /VCPKG_INSTALLATION_ROOT/u);
});
