SELECT add_continuous_aggregate_policy('monthly_revenue_by_application',
    start_offset => NULL,
    end_offset => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour');

SELECT add_continuous_aggregate_policy('monthly_revenue_by_developer',
    start_offset => NULL,
    end_offset => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour');

-- CALL refresh_continuous_aggregate('monthly_revenue_by_application', now() - INTERVAL '6 months', now());
-- CALL refresh_continuous_aggregate('monthly_revenue_by_developer', now() - INTERVAL '6 months', now());
