pub(super) struct DcheckFailure;

pub(super) fn _dcheck(f: bool) {
    if !f {
        std::panic::panic_any(DcheckFailure);
    }
}
