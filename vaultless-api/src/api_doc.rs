use crate::handlers::developer::application_dashboard::ApplicationResponse;
use utoipa::{OpenApi, Modify};
use utoipa::openapi::security::SecurityScheme;

#[derive(OpenApi)]
#[openapi(
    paths(
        // Application handlers
        crate::handlers::developer::application_dashboard::get_application_with_keys_handler,
        crate::handlers::developer::application::create_application,
        crate::handlers::developer::application::list_applications,
        crate::handlers::developer::application::update_application,
        crate::handlers::developer::application::deactivate_application,
        crate::handlers::developer::application::get_chart_data,
        crate::handlers::developer::application::get_user_usage_summary,
        crate::handlers::developer::application::get_quota_warnings,
        crate::handlers::developer::application::get_application_with_keys_handler,
        // Analytics handlers
        // crate::handlers::developer::analytics::get_application_quota_status,
        // crate::handlers::developer::analytics::get_application_cost_breakdown,
        crate::handlers::developer::analytics::export_application_usage,
        // crate::handlers::developer::analytics::get_application_trends,
        // User auth handlers
        crate::handlers::developer::user_auth::register,
        crate::handlers::developer::user_auth::login,
        crate::handlers::developer::user_auth::refresh_token,
        crate::handlers::developer::user_auth::logout,
        crate::handlers::developer::user_auth::verify_email_get,
        crate::handlers::developer::user_auth::verify_email_post,
        crate::handlers::developer::user_auth::resend_verification_email,
        crate::handlers::developer::user_auth::request_password_reset,
        crate::handlers::developer::user_auth::reset_password,
        crate::handlers::developer::user_auth::get_current_user,
        // Google OAuth handlers
        crate::handlers::developer::google_oauth::google_auth_init,
        crate::handlers::developer::google_oauth::google_auth_url,
        crate::handlers::developer::google_oauth::google_auth_callback,
        crate::handlers::developer::google_oauth::generate_test_token,
        // Notification handlers
        crate::handlers::developer::notification::list_notifications,
        crate::handlers::developer::notification::get_notification,
        crate::handlers::developer::notification::get_unread_count,
        crate::handlers::developer::notification::get_notification_summary,
        crate::handlers::developer::notification::mark_notification_read,
        crate::handlers::developer::notification::mark_all_notifications_read,
        crate::handlers::developer::notification::delete_notification,
        crate::handlers::developer::notification::delete_all_read_notifications,
    ),
    components(
        schemas(
            ApplicationResponse,
            // User auth schemas
            crate::handlers::developer::dto::RegisterRequest,
            crate::handlers::developer::dto::RegisterResponse,
            crate::handlers::developer::dto::LoginRequest,
            crate::handlers::developer::dto::LoginResponse,
            crate::handlers::developer::dto::UserInfo,
            crate::handlers::developer::dto::RefreshTokenRequest,
            crate::handlers::developer::dto::RefreshTokenResponse,
            crate::handlers::developer::dto::LogoutResponse,
            crate::handlers::developer::dto::VerifyEmailRequest,
            crate::handlers::developer::dto::VerifyEmailResponse,
            crate::handlers::developer::dto::RequestPasswordResetRequest,
            crate::handlers::developer::dto::RequestPasswordResetResponse,
            crate::handlers::developer::dto::ResetPasswordRequest,
            crate::handlers::developer::dto::ResetPasswordResponse,
            crate::handlers::developer::dto::CurrentUserResponse,
            crate::handlers::developer::dto::ResendVerificationRequest,
            crate::handlers::developer::user_auth::UserResponse,
            // Google OAuth schemas
            crate::handlers::developer::dto::GoogleAuthQuery,
            crate::handlers::developer::dto::GoogleAuthInitResponse,
            crate::handlers::developer::dto::GoogleCallbackQuery,
            crate::handlers::developer::dto::GoogleAuthResponse,
            crate::handlers::developer::dto::GoogleUserProfile,
            crate::handlers::developer::dto::GoogleAuthError,
            crate::handlers::developer::google_oauth::TestTokenRequest,
            crate::handlers::developer::google_oauth::TestTokenResponse,
            // Application schemas
            crate::handlers::developer::application::CreateApplicationRequest,
            crate::handlers::developer::application::CreateApplicationResponse,
            crate::handlers::developer::application::RealTimeUsageResponse,
            crate::handlers::developer::application::UpdateTierRequest,
            crate::handlers::developer::application::ApplicationResponse,
            crate::handlers::developer::application::QuotaWarningsQuery,
            crate::handlers::developer::application::PaginationParams,
            crate::handlers::developer::application::ChartQueryParams,
            vaultless_core::models::app_model::dto::PaginatedApplicationsSummary,
            vaultless_core::models::app_model::dto::ApplicationSummary,
            // Notification schemas
            vaultless_core::models::notification::Notification,
            vaultless_core::models::notification::NotificationType,
            vaultless_core::models::notification::NotificationSeverity,
            vaultless_core::models::notification::NotificationQuery,
            vaultless_core::models::notification::PaginatedNotifications,
            vaultless_core::models::notification::NotificationSummary,
            vaultless_core::models::notification::UnreadCountResponse,
            crate::handlers::developer::notification::MarkAllReadResponse,
            crate::handlers::developer::notification::DeleteResponse,
            crate::handlers::developer::notification::DeleteAllReadResponse,
            // Analytics schemas
            crate::handlers::developer::analytics::QuotaStatusResponse,
            crate::handlers::developer::analytics::CostBreakdownResponse,
            crate::handlers::developer::analytics::CostItem,
            crate::handlers::developer::analytics::TrendsResponse,
            crate::handlers::developer::analytics::ExportFormat,
            crate::handlers::developer::analytics::ExportQuery,
        )
    ),
    modifiers(&SecurityAddon),
    info(
        title = "Vaultless API",
        description = "Secure, privacy-first messaging platform API",
        contact(
            name = "Vaultless Support",
            email = "support@vaultless.com"
        ),
        license(
            name = "AGPL-3.0",
            url = "https://www.gnu.org/licenses/agpl-3.0.html"
        ),
        version = "1.0.0"
    )
)]
pub struct ApiDoc;

pub fn openapi_config() -> utoipa_swagger_ui::SwaggerUi {
    utoipa_swagger_ui::SwaggerUi::new("/dev-docs") // Path to serve Swagger UI
        .url("/api-docs/openapi.json", ApiDoc::openapi()) // Endpoint for the OpenAPI JSON schema
}

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.as_mut().unwrap();
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(utoipa::openapi::security::HttpBuilder::new()
                .bearer_format("JWT")
                .scheme(utoipa::openapi::security::HttpAuthScheme::Bearer)
                .build())
        );
    }
}

// =============================================================================
// Client API Documentation (Separate Swagger UI)
// =============================================================================

#[derive(OpenApi)]
#[openapi(
    paths(
        // Client Authentication
        crate::handlers::clients::auth::sign_up_client,
        crate::handlers::clients::auth::login_client,
        crate::handlers::clients::auth::generate_challenge,
        crate::handlers::clients::auth::lookup_client,
        crate::handlers::clients::auth::health_check,
        crate::handlers::clients::auth::get_current_client,
        crate::handlers::clients::auth::logout,
        crate::handlers::clients::auth::deactivate_client,
        // Instant Messaging
        crate::handlers::clients::instant_message::send_message,
        crate::handlers::clients::instant_message::fetch_inbox,
        crate::handlers::clients::instant_message::mark_message_read,
        crate::handlers::clients::instant_message::get_read_receipts,
        crate::handlers::clients::instant_message::message_health_check,
    ),
    components(
        schemas(
            // Client Auth schemas
            crate::handlers::clients::auth::LookupClientQuery,
            crate::handlers::clients::auth::ClientLookupResponse,
            crate::handlers::clients::auth::SuccessResponse,
            crate::handlers::clients::auth::ChallengeResponse,
            crate::middleware::client::ClientResponse,
            // Instant Message schemas
            crate::handlers::clients::instant_message::SendMessageRequest,
            crate::handlers::clients::instant_message::SendMessageResponse,
            crate::handlers::clients::instant_message::FetchMessagesResponse,
            crate::handlers::clients::instant_message::MarkReadResponse,
            crate::handlers::clients::instant_message::ReadReceiptsResponse,
            crate::handlers::clients::instant_message::HealthStatusResponse,
        )
    ),
    modifiers(&ClientSecurityAddon),
    info(
        title = "Vaultless Client API",
        description = "Client-side API for secure messaging - authentication and instant messaging endpoints",
        contact(
            name = "Vaultless Support",
            email = "support@vaultless.com"
        ),
        license(
            name = "AGPL-3.0",
            url = "https://www.gnu.org/licenses/agpl-3.0.html"
        ),
        version = "1.0.0"
    )
)]
pub struct ClientApiDoc;

struct ClientSecurityAddon;

impl Modify for ClientSecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.as_mut().unwrap();
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(utoipa::openapi::security::HttpBuilder::new()
                .bearer_format("JWT")
                .scheme(utoipa::openapi::security::HttpAuthScheme::Bearer)
                .build())
        );
    }
}

pub fn client_openapi_config() -> utoipa_swagger_ui::SwaggerUi {
    utoipa_swagger_ui::SwaggerUi::new("/client-docs")
        .url("/client-api-docs/openapi.json", ClientApiDoc::openapi())
}