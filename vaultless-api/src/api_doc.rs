use crate::handlers::developer::application_dashboard::ApplicationResponse;
use utoipa::{OpenApi, Modify};

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::handlers::developer::application_dashboard::get_application_with_keys_handler,
        crate::handlers::developer::application::create_application,
        crate::handlers::developer::application::list_applications,
        crate::handlers::developer::application::update_application,
        crate::handlers::developer::application::deactivate_application,
        crate::handlers::developer::application::get_chart_data,
        crate::handlers::developer::application::get_user_usage_summary,
        crate::handlers::developer::application::get_quota_warnings,
        crate::handlers::developer::application::get_application_with_keys_handler,
        crate::handlers::developer::user_auth::register,
        crate::handlers::developer::user_auth::login,
        crate::handlers::developer::user_auth::refresh_token,
        crate::handlers::developer::user_auth::logout,
        crate::handlers::developer::user_auth::verify_email_get,
        crate::handlers::developer::user_auth::verify_email_post,
        crate::handlers::developer::user_auth::request_password_reset,
        crate::handlers::developer::user_auth::reset_password,
        crate::handlers::developer::user_auth::get_current_user,
    ),
    components(
        schemas(
            ApplicationResponse,
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
            crate::handlers::developer::application::CreateApplicationRequest,
            crate::handlers::developer::application::RealTimeUsageResponse,
            crate::handlers::developer::application::UpdateTierRequest,
            crate::handlers::developer::application::ApplicationResponse,
            crate::handlers::developer::application::QuotaWarningsQuery,
            crate::handlers::developer::application::PaginationParams,
            crate::handlers::developer::application::ChartQueryParams,
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
    utoipa_swagger_ui::SwaggerUi::new("/docs") // Path to serve Swagger UI
        .url("/api-docs/openapi.json", ApiDoc::openapi()) // Endpoint for the OpenAPI JSON schema
}

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::SecurityScheme;

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