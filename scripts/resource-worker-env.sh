# Sourced only after the caller has verified its kernel resource boundary.
: "${RUST_TEST_THREADS:=1}"
for guard_key in CARGO_BUILD_JOBS RUST_TEST_THREADS RAYON_NUM_THREADS GOMAXPROCS OMP_NUM_THREADS OPENBLAS_NUM_THREADS MKL_NUM_THREADS CMAKE_BUILD_PARALLEL_LEVEL; do
  # Expansion uses fixed variable names, never command text from a project.
  eval "guard_value=\${$guard_key:-2}"
  case "$guard_value" in 1|2) ;; *) guard_value=2 ;; esac
  export "$guard_key=$guard_value"
done
unset CARGO_MAKEFLAGS guard_key guard_value
export MAKEFLAGS=-j2
