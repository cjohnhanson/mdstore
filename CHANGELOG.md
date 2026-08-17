# Changelog

## 0.3.2

### Changed

- `Error` is `#[non_exhaustive]`. A variant added to an exhaustive
  public enum is a major change, and this release adds one. Every
  consumer already matched with a wildcard arm; the attribute makes the
  compiler require it. A consumer matching exhaustively without a
  wildcard would not compile against 0.3.2, and none exists.

### Added

- `Error::StorePath { rel, root, source }`, carrying the io error a
  store path operation failed with. `InvalidStore` had a formatted
  string in its place.
- `Error::refused_by_confinement` and `Error::io_kind`. A consumer must
  tell a refusal from a fault: a directory the store refuses holds no
  documents, and a directory it cannot read is a fault that must not
  read as empty. cap-std reports both as PermissionDenied, and the
  errno is what separates them, so the error now carries it.

## 0.3.1

### Added

- `StoreDir::rename`, `StoreDir::remove_dir` and `StoreDir::dir_is_empty`.
  A consumer that moves a document between directories of one store had
  no confined way to do it, so it fell back to `std::fs` on a path it
  built. Both ends of a rename go through the handle. A rename replaces
  an existing destination, and moves a directory as well as a file.

  Additive, so the patch field moves. Under Cargo's 0.x rules the minor
  field is the breaking slot, and a bump there would make every consumer
  pinned at 0.3 edit a manifest for a change that only adds.

## 0.3.0

Breaking. A store now reads and writes through a capability handle, so
the operating system refuses a path that leaves the store. A caller
cannot forget a check, because there is no check to forget.

### Breaking changes

- `StoreContent::Dir` carries a `confined::StoreDir` in place of a
  `PathBuf`. Code that matched the variant for its path calls
  `StoreContent::dir()`, which still answers `Option<&Path>`.
- `ScanEntry.path` is relative to the store root. It was absolute. A
  consumer that passed it to `std::fs` reached the right file only
  while the store was the process's own directory tree.
- `store::read_document`, `store::write_document`,
  `store::is_regular_file` and `store::scan_documents` are gone. They
  built a path by joining and had no bound, so a caller that passed
  `../outside/secret.md` read it. `confined::StoreDir` replaces all
  four: `read`, `write`, `is_document`, `scan`. `store::document_dir`
  stays, and resolves the root a handle opens on.

### Fixes

- A write no longer removes a staging file it did not create. The
  error path destroyed another writer's staged content.
- A scan of a climbing path is refused. A missing leading directory
  made the operating system answer NotFound before it evaluated the
  climb, and the arm for a store with no document directory yet
  swallowed the refusal.
- A store holds one open handle. Re-opening the root per operation
  resolved it through ambient authority every time, so a root swapped
  between two calls was followed. Holding it also makes a scan of 500
  documents one root open rather than 501.
