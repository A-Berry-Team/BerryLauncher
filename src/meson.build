pkgdatadir = get_option('prefix') / get_option('datadir') / meson.project_name()
gnome = import('gnome')

# Manifest generated from src/resources/ (images).
gresource_xml = custom_target('berrylauncher-gresource-xml',
  output: 'berrylauncher.gresource.xml',
  command: [
    find_program('python3'),
    meson.project_source_root() / 'build-aux' / 'gen-gresource.py',
    meson.current_source_dir() / 'resources',
    '@OUTPUT@',
  ],
  build_always_stale: true,
)

gnome.compile_resources('berrylauncher',
  gresource_xml,
  source_dir: meson.current_source_dir(),
  gresource_bundle: true,
  install: true,
  install_dir: pkgdatadir,
)

conf = configuration_data()
conf.set_quoted('VERSION', meson.project_version())
conf.set_quoted('PKGDATADIR', pkgdatadir)

configure_file(
  input: 'config.rs.in',
  output: 'config.rs',
  configuration: conf
)

# Keep the generated config.rs in src/ (GNOME Builder's Rust template).
run_command(
  'cp',
  meson.project_build_root() / 'src' / 'config.rs',
  meson.project_source_root() / 'src' / 'config.rs',
  check: true,
)

cargo_bin = find_program('cargo')
cargo_manifest = meson.project_source_root() / 'Cargo.toml'
cargo_target_dir = meson.project_build_root() / 'src'
cargo_home = meson.project_build_root() / 'cargo-home'

# release and minsize builds use cargo's release profile; everything else the dev profile.
if get_option('buildtype') in ['release', 'minsize']
  cargo_mode = '--release'
  rust_target = 'release'
else
  cargo_mode = ''
  rust_target = 'debug'
endif

# Flags shared by every cargo invocation.
cargo_common = []
if get_option('locked')
  cargo_common += '--locked'
endif
if get_option('offline')
  cargo_common += '--offline'
endif
cargo_build_flags = ' '.join([cargo_mode] + cargo_common)

cargo_binary = cargo_target_dir / rust_target / meson.project_name()

# CARGO_HOME: honour the caller's (flatpak-builder points it at the vendored sources);
# fall back to a directory inside the build tree.
cargo_env_prefix = 'set -eu; CARGO_HOME="${CARGO_HOME:-$1}"; export CARGO_HOME;'

# Build with cargo, then copy the binary to Meson's output path.
cargo_build = custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: meson.project_name(),
  console: true,
  install: true,
  install_dir: get_option('bindir'),
  command: [
    'sh', '-c',
    cargo_env_prefix + ' "$2" build --manifest-path "$3" --target-dir "$4" $5; cp "$6" "$7"',
    'berrylauncher-cargo',
    cargo_home,
    cargo_bin,
    cargo_manifest,
    cargo_target_dir,
    cargo_build_flags,
    cargo_binary,
    '@OUTPUT@',
  ],
)

# Cargo.toml, meson.build and the newest AppStream <release> must agree.
test('Version consistency', find_program('python3'),
  args: [
    meson.project_source_root() / 'build-aux' / 'check-version.py',
    meson.project_version(),
    meson.project_source_root() / 'Cargo.toml',
    meson.project_source_root() / 'data' / 'app.berrylauncher.berry.metainfo.xml.in',
  ],
)

# Every symbolic icon the Rust code names must have a bundled SVG (see build-aux/check-icons.py).
test('Bundled symbolic icons', find_program('python3'),
  args: [
    meson.project_source_root() / 'build-aux' / 'check-icons.py',
    meson.project_source_root(),
  ],
)
