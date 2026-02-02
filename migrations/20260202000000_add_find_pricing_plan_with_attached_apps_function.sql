-- Add function to find pricing plan with attached applications

CREATE TYPE pagination_params AS (
    lim  INTEGER,
    off  INTEGER
);

CREATE OR REPLACE FUNCTION pagination_params(
    p_page INTEGER,
    p_page_size INTEGER
)
RETURNS pagination_params AS $$
DECLARE
    v_page INTEGER := GREATEST(COALESCE(p_page, 1), 1);
    v_page_size INTEGER := GREATEST(COALESCE(p_page_size, 0), 0);
BEGIN
    RETURN (
        v_page_size,
        (v_page - 1) * v_page_size
    )::pagination_params;
END;
$$ LANGUAGE plpgsql IMMUTABLE;

CREATE MATERIALIZED VIEW mv_attached_apps_by_plan AS
SELECT
    app_plan.pricing_plan_id,
    a.application_id,
    a.developer_id,
    a.name,
    a.is_active,
    a.created_at,
    a.quota_usage_percentage,
    a.bandwidth_quota_usage_percentage,
    a.current_month_revenue_cents
FROM mv_applications_with_usage a
JOIN application_pricing_plans app_plan
  ON a.application_id = app_plan.application_id;

CREATE INDEX idx_mv_attached_apps_plan_dev
    ON mv_attached_apps_by_plan (pricing_plan_id, developer_id);

CREATE INDEX idx_mv_attached_apps_created_at
    ON mv_attached_apps_by_plan (pricing_plan_id, created_at DESC);

REFRESH MATERIALIZED VIEW mv_attached_apps_by_plan;

CREATE OR REPLACE FUNCTION find_pricing_plan_with_attached_apps(
    p_plan_id UUID,
    p_developer_id UUID,
    p_attached_page INTEGER DEFAULT NULL,
    p_attached_page_size INTEGER DEFAULT NULL
)
RETURNS TABLE (
    plan_id UUID,
    plan_developer_id UUID,
    plan_name TEXT,
    plan_pricing_mode pricing_mode_enum,
    plan_price_per_message_cents BIGINT,
    plan_price_per_gb_cents BIGINT,
    plan_price_per_proof_cents BIGINT,
    plan_prepaid_amount_cents BIGINT,
    plan_created_at TIMESTAMPTZ,
    plan_attached_app_count BIGINT,
    attached_apps JSON
) AS $$
DECLARE
    pg pagination_params;
BEGIN
    pg := pagination_params(p_attached_page, p_attached_page_size);

    RETURN QUERY
    WITH plan_with_count AS (
        SELECT
            p.id,
            p.developer_id,
            p.name,
            p.pricing_mode,
            p.price_per_message_cents,
            p.price_per_gb_cents,
            p.price_per_proof_cents,
            p.prepaid_amount_cents,
            p.created_at,
            COALESCE(ac.attached_count, 0) AS attached_app_count
        FROM pricing_plans p
        LEFT JOIN (
            SELECT pricing_plan_id, COUNT(*) AS attached_count
            FROM application_pricing_plans
            GROUP BY pricing_plan_id
        ) ac ON p.id = ac.pricing_plan_id
        WHERE p.id = p_plan_id
          AND p.developer_id = p_developer_id
    ),
    attached_apps_base AS (
        SELECT *
        FROM mv_attached_apps_by_plan
        WHERE pricing_plan_id = p_plan_id
          AND developer_id = p_developer_id
    ),
    attached_apps_paged AS (
        SELECT *
        FROM attached_apps_base
        ORDER BY created_at DESC
        LIMIT pg.lim
        OFFSET pg.off
    ),
    attached_apps_json AS (
        SELECT
            CASE
                WHEN pg.lim > 0 THEN
                    json_build_object(
                        'items', (
                            SELECT json_agg(
                                json_build_object(
                                    'id', application_id,
                                    'name', name,
                                    'is_active', is_active,
                                    'created_at', created_at,
                                    'quota_usage_percentage', quota_usage_percentage,
                                    'bandwidth_quota_usage_percentage', bandwidth_quota_usage_percentage,
                                    'current_month_revenue_cents', current_month_revenue_cents
                                )
                                ORDER BY created_at DESC
                            )
                            FROM attached_apps_paged
                        ),
                        'total_count', (SELECT COUNT(*) FROM attached_apps_base),
                        'page', COALESCE(p_attached_page, 1),
                        'page_size', pg.lim,
                        'total_pages',
                            CEIL(
                                (SELECT COUNT(*) FROM attached_apps_base)::NUMERIC
                                / NULLIF(pg.lim, 0)
                            )::INTEGER
                    )
                ELSE NULL
            END AS apps_json
    )
    SELECT
        pw.id,
        pw.developer_id,
        pw.name,
        pw.pricing_mode,
        pw.price_per_message_cents,
        pw.price_per_gb_cents,
        pw.price_per_proof_cents,
        pw.prepaid_amount_cents,
        pw.created_at,
        pw.attached_app_count,
        aa.apps_json
    FROM plan_with_count pw
    CROSS JOIN attached_apps_json aa;
END;
$$ LANGUAGE plpgsql;

GRANT EXECUTE ON FUNCTION
find_pricing_plan_with_attached_apps(UUID, UUID, INTEGER, INTEGER)
TO vaultless;
