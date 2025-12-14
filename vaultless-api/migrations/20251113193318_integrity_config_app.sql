-- Add migration script here
ALTER TABLE public.applications
    -- Security & Configuration
    ADD COLUMN max_ttl_seconds integer NOT NULL DEFAULT 604800,
    ADD COLUMN is_key_rotation_forced boolean NOT NULL DEFAULT false,
    
    -- Auditing & Metrics
    ADD COLUMN deletion_requested_at timestamp with time zone,
    ADD COLUMN internal_notes text,

    -- dd the new, flexible JSONB column for all platform-specific security configuration
    ADD COLUMN app_meta jsonb NOT NULL DEFAULT '{
        "PlatformFingerPrint": {
            "browser": "00000000-0000-0000-0000-000000000000",
            "ios": "00000000-0000-0000-0000-000000000000",
            "android": "00000000-0000-0000-0000-000000000000",
            "iot": "00000000-0000-0000-0000-000000000000"
        },

        "IntegrityConfig": {
            "allow_unauthenticated": false,

            "browser": {
            "authorized_origins": ["https://app.example.com"],
            "require_origin_header": true,
            "require_referer_header": true,
            "cors_strict_mode": true,
            "require_captcha_on_registration": true,
            "captcha_provider": "turnstile",
            "captcha_site_key": null,
            "captcha_secret_key": null,
            "bind_client_to_origin": true,
            "track_origin_changes": true,
            "max_origin_changes_per_client": 3,
            "max_clients_per_ip": 5,
            "max_registrations_per_ip_per_hour": 10,
            "max_requests_per_ip_per_hour": 1000,
            "alert_on_usage_spike": true,
            "usage_spike_threshold": 2.0,
            "usage_baseline_hours": 24
            },

            "ios": {
            "apple_team_id": "ABCD123456",
            "allowed_bundle_ids": ["com.example.app"],
            "allowed_certificate_hashes": [],
            "min_version_code": 100,
            "reject_untrusted_device": true
            },

            "android": {
            "allowed_certificate_sha256": [],
            "allowed_package_names": ["com.example.app"],
            "min_version_code": 100,
            "reject_untrusted_device": true,
            "reject_unrecognized_version": true,
            "google_cloud_project": "project-123",
            "google_api_key": null,
            "max_token_age_seconds": 60
            },

            "iot": {
            "allowed_certificate_authorities": ["Example Root CA"],
            "require_valid_certificate_expiry": true,
            "reject_future_certificates": true,
            "require_cn_match": true
            },

            "rate_limits": {
            "max_attestations_per_user_per_hour": 100,
            "max_failed_attempts_before_lockout": 5
            }
        }
    }'::jsonb;

    -- Add an index to efficiently check which apps are due for key rotation
CREATE INDEX IF NOT EXISTS idx_applications_rotation_check
    ON public.applications (is_key_rotation_forced, updated_at)
    TABLESPACE pg_default
    WHERE is_key_rotation_forced = true;
    
-- Add an index for quick lookup of apps awaiting deletion
CREATE INDEX IF NOT EXISTS idx_applications_deletion_requested
    ON public.applications (deletion_requested_at)
    TABLESPACE pg_default
    WHERE deletion_requested_at IS NOT NULL;

-- Create GIN index for efficient querying within the JSONB column
CREATE INDEX IF NOT EXISTS idx_applications_app_meta_gin
    ON public.applications USING GIN (app_meta);

ALTER TABLE public.clients
ADD COLUMN IF NOT EXISTS is_platform_attested boolean NOT NULL DEFAULT false;

-- Optional: Add an index for faster lookups on active/attested clients
CREATE INDEX IF NOT EXISTS idx_clients_attested
    ON public.clients USING btree
    (application_id ASC NULLS LAST, is_platform_attested ASC NULLS LAST)
    TABLESPACE pg_default
    WHERE is_platform_attested = true;

    /* or "turnstile" */