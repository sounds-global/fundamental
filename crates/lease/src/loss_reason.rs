/// Why a holder stopped holding its lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LossReason {
    /// Another instance began a tenure.
    Superseded,
    /// It could not renew before its local deadline.
    Expired,
    /// It released the lease on a graceful stop.
    Released,
}
