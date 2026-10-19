# Protobuf sync

`protos/service.proto` defines the gRPC contract between this desktop app and
the Nodea backend. The backend lives in a separate, private repository (the backend repo), and that
repository is the **source of truth**: it generates the Python stubs from the same
file.

The copy in this repo is vendored. `src-tauri/build.rs` compiles it with
[`protoc-bin-vendored`](https://crates.io/crates/protoc-bin-vendored), so a bare
`git clone && cargo build` needs no `protoc` on the system.

## Changing the contract

Change it in the backend repo first, then copy the file here and commit it. That is
the whole procedure:

```bash
cp <backend-repo>/protos/service.proto protos/service.proto
git commit -m "Sync service.proto from the backend repo" protos/service.proto
```

Deliberately manual. A proto change usually needs matching work on both sides,
and there is no way to land that atomically across two repositories, so the
copy is a decision someone makes rather than something that happens.

## Drift detection

There is none in this repository. The backend repo is private, so a check here
would need a credential with read access to it, and a check that fork PRs
cannot run is noise. A contract change is caught when the two sides are next
tested together.

## Why the package is still `ai4all.v1`

The proto package (`ai4all.v1`) and service (`AI4AllService`) predate the
rename to Nodea. They are part of the wire protocol - the gRPC method paths are
built from them - so renaming them would break every deployed client and
server at once. They stay until a versioned `v2` of the contract is worth the
migration.
