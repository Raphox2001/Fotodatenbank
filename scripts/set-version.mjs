// Setzt die Versionsnummer in package.json, tauri.conf.json und Cargo.toml.
// Aufruf: node scripts/set-version.mjs 0.3.0
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("Bitte eine Version wie 0.3.0 angeben");
  process.exit(1);
}

for (const file of ["package.json", "src-tauri/tauri.conf.json"]) {
  const json = JSON.parse(readFileSync(file, "utf8"));
  json.version = version;
  writeFileSync(file, JSON.stringify(json, null, 2) + "\n");
}

const cargo = "src-tauri/Cargo.toml";
writeFileSync(cargo, readFileSync(cargo, "utf8").replace(/^version = "[^"]+"/m, `version = "${version}"`));
console.log(`Version ${version} gesetzt`);
