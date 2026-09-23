# Stable feature snapshot (1.88-1.98)

Verified 2026-09-23 against official Rust release notes. Re-verify against
[RELEASES.md](https://github.com/rust-lang/rust/blob/master/RELEASES.md) or
[std docs](https://doc.rust-lang.org/std/) for anything newer or uncertain.

## 1.88

- Let chains in `if`/`while` (edition 2024) - replaces nested `if let`.
- `<[T]>::as_chunks[_mut]`, `as_rchunks[_mut]` - replaces manual slice
  splitting with bounds math.
- `HashMap::extract_if`, `HashSet::extract_if` - in-place filtered drain.
- `Cell::update` - replaces get/modify/set triples.
- `#[cfg(true)]` / `#[cfg(false)]` literals; naked functions.
- Cargo: automatic garbage collection of the global cache.

## 1.89

- `_` const-generic argument inference - `[false; _]` inside generic fns.
- `File::lock` / `lock_shared` / `try_lock*` / `unlock` - replaces ad-hoc
  file locking.
- `NonNull::from_ref` / `from_mut`; `NonZero<char>`; `Result::flatten`.
- `OsString::leak`, `PathBuf::leak`; `format_args!` values storable in a
  variable.
- `mismatched_lifetime_syntaxes` lint - write `Iter<'_, T>` when inputs use
  `'_`.
- Cargo: `cargo fix` / `cargo clippy --fix` fix only selected targets;
  doctests are tested when cross-compiling.

## 1.90

- `uN::{checked,overflowing,saturating,wrapping}_sub_signed` - replaces
  widening casts for unsigned-minus-signed.
- `CStr`/`CString`/`Cow<CStr>` equality impls.
- Const `f32`/`f64` rounding methods; `lld` is the default linker on
  x86_64 Linux.

## 1.91

- `Path::file_prefix`; `PathBuf::add_extension`, `with_added_extension`;
  `Path`/`PathBuf` equality with `str`/`String`.
- `Duration::from_mins`, `from_hours`; `str::ceil_char_boundary`,
  `floor_char_boundary`.
- Strict arithmetic family (`strict_add` ... `strict_pow`) - panics instead
  of wrapping in debug and release.
- `core::iter::chain` free function; `core::array::repeat`.
- `BTreeMap::extract_if`, `BTreeSet::extract_if`; `Cell::as_array_of_cells`.
- `uN::carrying_add` / `borrowing_sub` / `carrying_mul(_add)` -
  multi-precision arithmetic without external crates.
- Cargo: `build.build-dir` config; `--target host-tuple`.

## 1.92

- `RwLockWriteGuard::downgrade` - replaces drop-plus-read_lock.
- `Box/Rc/Arc::new_zeroed[_slice]`; `NonZero::div_ceil`;
  `btree_map::Entry::insert_entry`; `Location::file_as_c_str`.
- Cargo book gained the "Optimizing Build Performance" chapter.

## 1.93

- `std::fmt::from_fn` / `FromFn` - one-off `Display`/`Debug` via closure.
- `Vec::into_raw_parts`, `String::into_raw_parts`.
- `<[T]>::as_array`, `as_mut_array` (and raw-slice forms) - length-checked
  slice-to-array conversion.
- `VecDeque::pop_front_if`, `pop_back_if`; `Duration::from_nanos_u128`;
  `char::MAX_LEN_UTF8` / `MAX_LEN_UTF16`.
- MaybeUninit slice API (`assume_init_ref/mut/drop`,
  `write_copy_of_slice`, `write_clone_of_slice`).
- Cargo: `cargo clean --workspace`; per-profile `CARGO_CFG_DEBUG_ASSERTIONS`
  in build scripts.

## 1.94

- `<[T]>::array_windows` - overlapping fixed-size windows without slicing.
- `LazyLock::get` / `get_mut` / `force_mut` (and `LazyCell`) - inspect
  initialized state without forcing initialization.
- `Peekable::next_if_map[_mut]`; `TryFrom<char> for usize`.
- Cargo: config `include` key for shared configuration files; TOML v1.1
  manifest parsing (raises development MSRV only); `CARGO_BIN_EXE_<crate>`
  usable at runtime.
- `dead_code` now inherits from traits; new `unused_visibilities` lint.

## 1.95

- `if let` guards on match arms - removes nested match boilerplate.
- `cfg_select!` - replaces `cfg_if!`-style chains with builtin syntax.
- `Vec::push_mut`, `Vec::insert_mut` (plus VecDeque/LinkedList forms) -
  return `&mut T` to the inserted element, replacing push-then-index.
- `Atomic::{update,try_update}` across atomics - fetch_update without
  `Ordering` ceremony.
- `bool::try_from({integer})`; `core::hint::cold_path`;
  `Layout::repeat[_packed]`; `ptr::{as_ref_unchecked,as_mut_unchecked}`;
  `core::range` module.

## 1.96

- `assert_matches!`, `debug_assert_matches!` - replaces `matches!` plus
  `assert!` in tests.
- `From<T> for LazyLock/LazyCell` - initialized-now singletons without
  closures.
- Ranges of `NonZero` integers iterate; more `core::range` types;
  `{core,std}::derive` path (MSRV 1.96).

## 1.97

- Integer bit helpers: `highest_one`, `lowest_one`, `isolate_highest_one`,
  `isolate_lowest_one`, `bit_width` (plus `NonZero` forms) - replaces
  `leading_zeros`/trailing shift tricks and some bitfield crates.
- `dead_code_pub_in_binary` lint (allow-by-default) - enable in binary crates
  to find unused `pub` items.
- Cargo: `build.warnings` config enforces warning-free local packages
  without `-Dwarnings` on the command line; `resolver.lockfile-path`; `-m`
  shorthand for `--manifest-path`; `cargo clean` refuses non-target dirs.
- v0 symbol mangling becomes the default - older debuggers/profilers may
  fail to demangle.

## 1.98

- `str::substr_range`, `<[T]>::subslice_range` - index ranges for
  subslices/substrings; ideal for parsers returning spans.
- `core::fmt::NumBuffer` plus `{integer}::format_into` - allocation-free
  integer formatting.
- `str::strip_circumfix`, `<[T]>::strip_circumfix` - strips a matching
  prefix/suffix pair.
- `String::from_utf16le/be[_lossy]`; `NonZero::from_str_radix`;
  algebraic float ops; `Atomic::from_mut(_slice)`, `get_mut_slice` - stack
  atomics over `&mut` data.
- `std::range::legacy` conversions; rustfmt discovers `cfg_select!` modules.

## Older but frequently missed

- `Option::is_some_and` (1.70), let-else (1.65), inline format args `{x}`
  (1.58), `OnceLock` (1.70), `div_ceil` (1.73), `array::from_fn` (1.63),
  `slice::first_chunk` (1.77), `#[expect]` (1.81), `error_in_core` (1.81),
  `LazyLock` (1.80), trait upcasting (1.86), `impl Trait` return position in
  traits (1.75).
