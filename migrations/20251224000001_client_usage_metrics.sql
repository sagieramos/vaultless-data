-- =============================================================================
-- Client Usage Metrics (Billable, Application-scoped)
-- TimescaleDB hypertable
-- =============================================================================

CREATE TABLE IF NOT EXISTS public.client_usage_metrics (
    period_start timestamptz NOT NULL,
    period_end timestamptz NOT NULL,

    application_id uuid NOT NULL,
    client_id uuid NOT NULL,

    -- Usage counters
    messages_sent bigint NOT NULL DEFAULT 0,
    messages_received bigint NOT NULL DEFAULT 0,
    proofs_verified bigint NOT NULL DEFAULT 0,
    total_bytes_stored bigint NOT NULL DEFAULT 0,
    total_bytes_sent bigint NOT NULL DEFAULT 0,
    total_bytes_received bigint NOT NULL DEFAULT 0,
    rate_limit_hits integer NOT NULL DEFAULT 0,

    created_at timestamptz NOT NULL DEFAULT now(),

    -- Enforce one row per client per application per period
    CONSTRAINT client_usage_unique_period
        UNIQUE (application_id, client_id, period_start),

    CONSTRAINT client_usage_valid_period
        CHECK (period_end > period_start),

    CONSTRAINT client_usage_valid_counters CHECK (
        messages_sent >= 0 AND
        messages_received >= 0 AND
        proofs_verified >= 0 AND
        total_bytes_stored >= 0 AND
        total_bytes_sent >= 0 AND
        total_bytes_received >= 0 AND
        rate_limit_hits >= 0
    ),

    CONSTRAINT client_usage_client_fkey
        FOREIGN KEY (client_id)
        REFERENCES public.clients(id)
        ON DELETE CASCADE,

    CONSTRAINT client_usage_application_fkey
        FOREIGN KEY (application_id)
        REFERENCES public.applications(id)
        ON DELETE CASCADE
);

ALTER TABLE public.client_usage_metrics
    OWNER TO vaultless;

-- =============================================================================
-- TimescaleDB setup
-- =============================================================================

-- Convert to hypertable
SELECT create_hypertable(
    'public.client_usage_metrics',
    'period_start',
    if_not_exists => TRUE
);

-- =============================================================================
-- Indexes
-- =============================================================================

-- Application-level lookups (invoices, dashboards)
CREATE INDEX IF NOT EXISTS idx_client_usage_application_period
ON public.client_usage_metrics (application_id, period_start DESC);

-- Client-level dashboards
CREATE INDEX IF NOT EXISTS idx_client_usage_client_period
ON public.client_usage_metrics (client_id, period_start DESC);


-- Create a continuous aggregate view
CREATE MATERIALIZED VIEW public.client_usage_monthly
WITH (timescaledb.continuous) AS
SELECT
    application_id,
    client_id,
    time_bucket('30 days', period_start) AS period_start,
    max(period_end) AS period_end,
    sum(messages_sent) AS messages_sent,
    sum(messages_received) AS messages_received,
    sum(proofs_verified) AS proofs_verified,
    sum(total_bytes_stored) AS total_bytes_stored,
    sum(total_bytes_sent) AS total_bytes_sent,
    sum(total_bytes_received) AS total_bytes_received,
    sum(rate_limit_hits) AS rate_limit_hits
FROM public.client_usage_metrics
GROUP BY application_id, client_id, time_bucket('30 days', period_start)
WITH NO DATA;
