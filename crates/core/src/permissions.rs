//! Permissions (spec 001b-1 req. 6, `07-security-compliance`).
//!
//! Permissions are fine-grained strings stored on the `role` row and
//! checked in the core — never only in the UI. The checker is generic;
//! the checkout permissions are the first users.

use crate::users::Role;

/// Create a sale at the till.
pub const PERM_SALE_CREATE: &str = "sale.create";
/// Discount one line (subject to the role's `max_line_discount_bps`).
pub const PERM_SALE_DISCOUNT_LINE: &str = "sale.discount.line";
/// Discount the whole bill.
pub const PERM_SALE_DISCOUNT_BILL: &str = "sale.discount.bill";
/// Override a price.
pub const PERM_SALE_PRICE_OVERRIDE: &str = "sale.price_override";
/// Void a line.
pub const PERM_SALE_VOID_LINE: &str = "sale.void_line";
/// Create a return.
pub const PERM_RETURN_CREATE: &str = "return.create";
/// Create a return without a receipt.
pub const PERM_RETURN_NO_RECEIPT: &str = "return.no_receipt";
/// Close a shift.
pub const PERM_SHIFT_CLOSE: &str = "shift.close";
/// Open the cash drawer manually.
pub const PERM_CASH_DRAWER_OPEN: &str = "cash.drawer_open";
/// Adjust stock.
pub const PERM_STOCK_ADJUST: &str = "stock.adjust";
/// Approve a stocktake.
pub const PERM_STOCK_STOCKTAKE_APPROVE: &str = "stock.stocktake.approve";
/// Edit the catalog.
pub const PERM_CATALOG_EDIT: &str = "catalog.edit";
/// Edit a price.
pub const PERM_PRICE_EDIT: &str = "price.edit";
/// View reports for this branch.
pub const PERM_REPORT_VIEW_BRANCH: &str = "report.view.branch";
/// View reports for all branches.
pub const PERM_REPORT_VIEW_ALL: &str = "report.view.all";
/// Manage users and roles.
pub const PERM_USER_MANAGE: &str = "user.manage";
/// Edit settings.
pub const PERM_SETTINGS_EDIT: &str = "settings.edit";
/// Restore a backup.
pub const PERM_BACKUP_RESTORE: &str = "backup.restore";

/// Every permission in the 07 list.
pub const ALL_PERMISSIONS: &[&str] = &[
    PERM_SALE_CREATE,
    PERM_SALE_DISCOUNT_LINE,
    PERM_SALE_DISCOUNT_BILL,
    PERM_SALE_PRICE_OVERRIDE,
    PERM_SALE_VOID_LINE,
    PERM_RETURN_CREATE,
    PERM_RETURN_NO_RECEIPT,
    PERM_SHIFT_CLOSE,
    PERM_CASH_DRAWER_OPEN,
    PERM_STOCK_ADJUST,
    PERM_STOCK_STOCKTAKE_APPROVE,
    PERM_CATALOG_EDIT,
    PERM_PRICE_EDIT,
    PERM_REPORT_VIEW_BRANCH,
    PERM_REPORT_VIEW_ALL,
    PERM_USER_MANAGE,
    PERM_SETTINGS_EDIT,
    PERM_BACKUP_RESTORE,
];

/// Does this role hold `permission`? The check is exact-match on the
/// permission string; there is no wildcard.
#[must_use]
pub fn has_permission(role: &Role, permission: &str) -> bool {
    role.permissions.iter().any(|p| p == permission)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn role(permissions: &[&str]) -> Role {
        Role {
            id: "r1".to_string(),
            tenant_id: "t1".to_string(),
            name: "Test".to_string(),
            permissions: permissions.iter().map(|p| (*p).to_string()).collect(),
            max_line_discount_bps: 500,
        }
    }

    #[test]
    fn checker_is_exact_match() {
        let cashier = role(&[PERM_SALE_CREATE, PERM_SALE_DISCOUNT_LINE]);
        assert!(has_permission(&cashier, PERM_SALE_CREATE));
        assert!(has_permission(&cashier, PERM_SALE_DISCOUNT_LINE));
        assert!(!has_permission(&cashier, PERM_SALE_DISCOUNT_BILL));
        assert!(!has_permission(&cashier, "sale.create.extra"));
        assert!(!has_permission(&cashier, "sale"));
        let none = role(&[]);
        assert!(!has_permission(&none, PERM_SALE_CREATE));
    }
}
