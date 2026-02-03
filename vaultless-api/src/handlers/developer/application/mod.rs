//! Application management handlers.
//!
//! This module provides handlers for:
//! - Application CRUD operations
//! - Key rotation (secret and publishable keys)
//! - Chart data for analytics dashboards

pub mod charts;
pub mod dto;
pub mod handlers;
pub mod keys;

// Re-export handlers for easy access
pub use charts::get_chart_data;
pub use handlers::{
    create_application, deactivate_application, get_application_analytics,
    get_quota_warnings, get_user_usage_summary, list_applications,
    update_application,
};
pub use keys::{
    add_publishable_key, deactivate_publishable_key, rotate_publishable_key, rotate_secret_key,
};
