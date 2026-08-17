# Changelog

## 0.4.0

### Added

- `StoreDir::rename`, `StoreDir::remove_dir` and `StoreDir::dir_is_empty`.
  A consumer that moves a document between directories of one store had
  no confined way to do it, so it fell back to `std::fs` on a path it
  built. Both ends of a rename go through the handle.

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
