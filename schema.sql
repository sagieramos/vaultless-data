--
-- PostgreSQL database dump
--

-- Dumped from database version 16.3
-- Dumped by pg_dump version 16.3

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: timescaledb; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS timescaledb WITH SCHEMA public;


--
-- Name: EXTENSION timescaledb; Type: COMMENT; Schema: -; Owner: 
--

COMMENT ON EXTENSION timescaledb IS 'Enables scalable inserts and complex queries for time-series data (Community Edition)';


--
-- Name: uuid-ossp; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS "uuid-ossp" WITH SCHEMA public;


--
-- Name: EXTENSION "uuid-ossp"; Type: COMMENT; Schema: -; Owner: 
--

COMMENT ON EXTENSION "uuid-ossp" IS 'generate universally unique identifiers (UUIDs)';


--
-- Name: group_type_enum; Type: TYPE; Schema: public; Owner: vaultless
--

CREATE TYPE public.group_type_enum AS ENUM (
    'private',
    'public',
    'broadcast'
);


ALTER TYPE public.group_type_enum OWNER TO vaultless;

--
-- Name: iot_device_status; Type: TYPE; Schema: public; Owner: vaultless
--

CREATE TYPE public.iot_device_status AS ENUM (
    'active',
    'revoked',
    'suspended',
    'decommissioned'
);


ALTER TYPE public.iot_device_status OWNER TO vaultless;

--
-- Name: key_type; Type: TYPE; Schema: public; Owner: vaultless
--

CREATE TYPE public.key_type AS ENUM (
    'secret',
    'publishable'
);


ALTER TYPE public.key_type OWNER TO vaultless;

--
-- Name: member_role_enum; Type: TYPE; Schema: public; Owner: vaultless
--

CREATE TYPE public.member_role_enum AS ENUM (
    'admin',
    'moderator',
    'member'
);


ALTER TYPE public.member_role_enum OWNER TO vaultless;

--
-- Name: member_status_enum; Type: TYPE; Schema: public; Owner: vaultless
--

CREATE TYPE public.member_status_enum AS ENUM (
    'active',
    'muted',
    'left',
    'removed',
    'banned'
);


ALTER TYPE public.member_status_enum OWNER TO vaultless;

--
-- Name: notification_severity; Type: TYPE; Schema: public; Owner: vaultless
--

CREATE TYPE public.notification_severity AS ENUM (
    'info',
    'warning',
    'critical'
);


ALTER TYPE public.notification_severity OWNER TO vaultless;

--
-- Name: notification_type; Type: TYPE; Schema: public; Owner: vaultless
--

CREATE TYPE public.notification_type AS ENUM (
    'quota_warning',
    'quota_exceeded',
    'billing_alert',
    'security_alert',
    'system_update',
    'marketing_offer',
    'api_key_expiring',
    'usage_report'
);


ALTER TYPE public.notification_type OWNER TO vaultless;

--
-- Name: pricing_mode_enum; Type: TYPE; Schema: public; Owner: vaultless
--

CREATE TYPE public.pricing_mode_enum AS ENUM (
    'postpaid',
    'prepaid',
    'free'
);


ALTER TYPE public.pricing_mode_enum OWNER TO vaultless;

--
-- Name: subscription_status_enum; Type: TYPE; Schema: public; Owner: vaultless
--

CREATE TYPE public.subscription_status_enum AS ENUM (
    'active',
    'paused',
    'cancelled'
);


ALTER TYPE public.subscription_status_enum OWNER TO vaultless;

--
-- Name: subscription_tier; Type: TYPE; Schema: public; Owner: vaultless
--

CREATE TYPE public.subscription_tier AS ENUM (
    'free',
    'starter',
    'pro',
    'enterprise'
);


ALTER TYPE public.subscription_tier OWNER TO vaultless;

--
-- Name: add_group_member(uuid, uuid, uuid); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.add_group_member(p_group_id uuid, p_client_address uuid, p_invited_by uuid) RETURNS uuid
    LANGUAGE plpgsql
    AS $$
DECLARE
    v_member_id UUID;
BEGIN
    INSERT INTO group_members (
        group_id,
        client_address,
        invited_by_client_address,
        status,
        joined_at
    )
    VALUES (p_group_id, p_client_address, p_invited_by, 'active', NOW())
    RETURNING id INTO v_member_id;

    RETURN v_member_id;
EXCEPTION WHEN unique_violation THEN
    SELECT id INTO v_member_id FROM group_members WHERE group_id = p_group_id AND client_address = p_client_address;
    RETURN v_member_id;
END;
$$;


ALTER FUNCTION public.add_group_member(p_group_id uuid, p_client_address uuid, p_invited_by uuid) OWNER TO vaultless;

--
-- Name: check_group_key_rotation(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.check_group_key_rotation() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- When a member leaves, removed, or banned, log a notification
    -- (In production, you might want to send this to a queue or notification system)
    IF (NEW.status IN ('left', 'removed', 'banned')) AND 
       (OLD.status = 'active') THEN
        
        -- Insert into a notifications table or log
        RAISE NOTICE 'Member % left group %. Consider rotating group encryption key.', 
            NEW.client_address, NEW.group_id;
        
        -- You could also insert into a key_rotation_queue table here
        -- INSERT INTO key_rotation_queue (group_id, reason, created_at)
        -- VALUES (NEW.group_id, 'member_left', NOW());
    END IF;

    RETURN NEW;
END;
$$;


ALTER FUNCTION public.check_group_key_rotation() OWNER TO vaultless;

--
-- Name: cleanup_expired_files(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.cleanup_expired_files() RETURNS TABLE(deleted_files bigint, deleted_chunks bigint)
    LANGUAGE plpgsql
    AS $$
DECLARE
    v_deleted_files BIGINT;
    v_deleted_chunks BIGINT;
BEGIN
    -- Delete expired files
    WITH deleted AS (
        DELETE FROM group_files
        WHERE expires_at < NOW()
        RETURNING id
    )
    SELECT COUNT(*) INTO v_deleted_files FROM deleted;
    
    -- Delete orphaned chunks (files that were deleted)
    WITH deleted AS (
        DELETE FROM file_chunks
        WHERE file_id NOT IN (SELECT id FROM group_files)
        RETURNING id
    )
    SELECT COUNT(*) INTO v_deleted_chunks FROM deleted;
    
    RETURN QUERY SELECT v_deleted_files, v_deleted_chunks;
END;
$$;


ALTER FUNCTION public.cleanup_expired_files() OWNER TO vaultless;

--
-- Name: cleanup_expired_messages(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.cleanup_expired_messages() RETURNS integer
    LANGUAGE plpgsql
    AS $$
DECLARE
    deleted_count INTEGER;
BEGIN
    DELETE FROM messages WHERE expires_at < NOW();
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN deleted_count;
END;
$$;


ALTER FUNCTION public.cleanup_expired_messages() OWNER TO vaultless;

--
-- Name: cleanup_expired_notifications(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.cleanup_expired_notifications() RETURNS TABLE(deleted_count bigint)
    LANGUAGE plpgsql
    AS $$
BEGIN
    DELETE FROM notifications 
    WHERE expires_at IS NOT NULL 
        AND expires_at < NOW();
    
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN NEXT;
END;
$$;


ALTER FUNCTION public.cleanup_expired_notifications() OWNER TO vaultless;

--
-- Name: cleanup_expired_refresh_tokens(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.cleanup_expired_refresh_tokens() RETURNS integer
    LANGUAGE plpgsql
    AS $$
DECLARE
    deleted_count INTEGER;
BEGIN
    DELETE FROM refresh_tokens 
    WHERE expires_at < NOW() 
        OR (is_used = true AND created_at < NOW() - INTERVAL '7 days');
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN deleted_count;
END;
$$;


ALTER FUNCTION public.cleanup_expired_refresh_tokens() OWNER TO vaultless;

--
-- Name: cleanup_expired_sessions(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.cleanup_expired_sessions() RETURNS integer
    LANGUAGE plpgsql
    AS $$
DECLARE
    deleted_count INTEGER;
BEGIN
    DELETE FROM user_sessions WHERE expires_at < NOW();
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN deleted_count;
END;
$$;


ALTER FUNCTION public.cleanup_expired_sessions() OWNER TO vaultless;

--
-- Name: cleanup_expired_sessions_crypto(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.cleanup_expired_sessions_crypto() RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
    UPDATE public.session_keys
    SET is_active = false
    WHERE expires_at < NOW() AND is_active = true;
END;
$$;


ALTER FUNCTION public.cleanup_expired_sessions_crypto() OWNER TO vaultless;

--
-- Name: FUNCTION cleanup_expired_sessions_crypto(); Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON FUNCTION public.cleanup_expired_sessions_crypto() IS 'Scheduled cleanup job. Remove sessions expired for 7+ days.';


--
-- Name: cleanup_old_login_attempts(integer); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.cleanup_old_login_attempts(retention_days integer) RETURNS integer
    LANGUAGE plpgsql
    AS $$
DECLARE
    deleted_count INTEGER;
BEGIN
    DELETE FROM login_attempts
    WHERE created_at < NOW() - (retention_days || ' days')::INTERVAL;
    
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN deleted_count;
END;
$$;


ALTER FUNCTION public.cleanup_old_login_attempts(retention_days integer) OWNER TO vaultless;

--
-- Name: cleanup_old_read_notifications(integer); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.cleanup_old_read_notifications(retention_days integer) RETURNS TABLE(deleted_count bigint)
    LANGUAGE plpgsql
    AS $$
BEGIN
    DELETE FROM notifications 
    WHERE is_read = TRUE 
        AND read_at < NOW() - (retention_days || ' days')::INTERVAL;
    
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN NEXT;
END;
$$;


ALTER FUNCTION public.cleanup_old_read_notifications(retention_days integer) OWNER TO vaultless;

--
-- Name: cleanup_reactions_on_message_delete(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.cleanup_reactions_on_message_delete() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    DELETE FROM message_reactions WHERE message_id = OLD.id;
    RETURN OLD;
END;
$$;


ALTER FUNCTION public.cleanup_reactions_on_message_delete() OWNER TO vaultless;

--
-- Name: cleanup_sender_keys_on_member_leave(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.cleanup_sender_keys_on_member_leave() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.status IN ('left', 'removed', 'banned') AND OLD.status = 'active' THEN
        -- Delete sender keys for this member
        DELETE FROM sender_keys
        WHERE group_id = NEW.group_id 
            AND (sender_client_id = NEW.client_address 
                 OR recipient_client_id = NEW.client_address);
    END IF;
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.cleanup_sender_keys_on_member_leave() OWNER TO vaultless;

--
-- Name: create_message_group(uuid, character varying, public.group_type_enum); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.create_message_group(p_creator_address uuid, p_group_name character varying, p_group_type public.group_type_enum DEFAULT 'private'::public.group_type_enum) RETURNS uuid
    LANGUAGE plpgsql
    AS $$
DECLARE
    v_group_id UUID;
BEGIN
    INSERT INTO message_groups (
        creator_client_address,
        group_name,
        group_type,
        created_at, updated_at
    )
    VALUES (p_creator_address, p_group_name, p_group_type, NOW(), NOW())
    RETURNING id INTO v_group_id;

    INSERT INTO group_members (
        group_id,
        client_address,
        role,
        can_add_members,
        can_remove_members,
        status,
        joined_at
    )
    VALUES (
        v_group_id,
        p_creator_address,
        'admin',
        TRUE,
        TRUE,
        'active',
        NOW()
    );

    UPDATE message_groups
    SET member_count = 1, updated_at = NOW()
    WHERE id = v_group_id;

    RETURN v_group_id;
END;
$$;


ALTER FUNCTION public.create_message_group(p_creator_address uuid, p_group_name character varying, p_group_type public.group_type_enum) OWNER TO vaultless;

--
-- Name: deactivate_inactive_clients(integer); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.deactivate_inactive_clients(p_inactive_days integer DEFAULT 90) RETURNS integer
    LANGUAGE plpgsql
    AS $$
DECLARE
    v_deactivated_count INTEGER;
BEGIN
    UPDATE clients
    SET is_active = FALSE
    WHERE 
        last_seen_at < NOW() - (p_inactive_days || ' days')::INTERVAL
        AND is_active = TRUE;
    
    GET DIAGNOSTICS v_deactivated_count = ROW_COUNT;
    RETURN v_deactivated_count;
END;
$$;


ALTER FUNCTION public.deactivate_inactive_clients(p_inactive_days integer) OWNER TO vaultless;

--
-- Name: FUNCTION deactivate_inactive_clients(p_inactive_days integer); Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON FUNCTION public.deactivate_inactive_clients(p_inactive_days integer) IS 'Optional GDPR compliance. Deactivate clients inactive for N days.';


--
-- Name: fetch_auth_config_by_publishable_key(text); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.fetch_auth_config_by_publishable_key(pk_plaintext text) RETURNS TABLE(app_id uuid, app_user_id uuid, app_name character varying, app_description text, app_is_active boolean, app_max_ttl_seconds integer, app_is_key_rotation_forced boolean, app_meta jsonb, sk_id uuid, sk_key_prefix character varying, sub_tier public.subscription_tier, sub_monthly_message_quota bigint, sub_message_retention_seconds bigint, sub_rate_limit_per_minute integer)
    LANGUAGE sql STABLE
    AS $$
    -- Step 1: Find the Application linked to this Publishable Key
    WITH app_lookup AS (
        SELECT application_id
        FROM public.api_keys
        WHERE publishable_key_plaintext = pk_plaintext
          AND key_type = 'publishable'::key_type
          AND is_active = TRUE
        LIMIT 1
    )
    -- Step 2: Join Application -> Subscription AND Application -> Active Secret Key
    SELECT
        a.id, a.developer_id, a.name, a.description, a.is_active,
        a.max_ttl_seconds, a.is_key_rotation_forced, a.app_meta,
        sk.id, sk.key_prefix,
        s.tier, s.monthly_message_quota, s.message_retention_seconds,
        s.rate_limit_per_minute
    FROM public.applications a
    JOIN app_lookup al ON a.id = al.application_id
    -- Link via application.subscription_id
    JOIN public.developer_subscriptions s ON a.subscription_id = s.id
    -- Optional: Get the current active secret key if one exists
    LEFT JOIN public.api_keys sk ON a.id = sk.application_id 
        AND sk.key_type = 'secret'::key_type 
        AND sk.is_active = TRUE
    WHERE a.is_active = TRUE
      AND s.is_active = TRUE
    LIMIT 1;
$$;


ALTER FUNCTION public.fetch_auth_config_by_publishable_key(pk_plaintext text) OWNER TO vaultless;

--
-- Name: fetch_auth_config_by_secret_hash(text); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.fetch_auth_config_by_secret_hash(sk_hash_hex text) RETURNS TABLE(app_id uuid, app_user_id uuid, app_name character varying, app_description text, app_is_active boolean, app_max_ttl_seconds integer, app_is_key_rotation_forced boolean, app_meta jsonb, sk_id uuid, sk_key_prefix character varying, sub_tier public.subscription_tier, sub_monthly_message_quota bigint, sub_message_retention_seconds bigint, sub_rate_limit_per_minute integer)
    LANGUAGE sql STABLE
    AS $$
    -- Direct chain: API Key -> Application -> Subscription
    SELECT
        a.id, a.developer_id, a.name, a.description, a.is_active,
        a.max_ttl_seconds, a.is_key_rotation_forced, a.app_meta,
        sk.id, sk.key_prefix,
        s.tier, s.monthly_message_quota, s.message_retention_seconds,
        s.rate_limit_per_minute
    FROM public.api_keys sk
    INNER JOIN public.applications a ON sk.application_id = a.id
    -- Link via application.subscription_id
    INNER JOIN public.developer_subscriptions s ON a.subscription_id = s.id
    WHERE sk.key_hash = sk_hash_hex
      AND sk.key_type = 'secret'::key_type
      AND sk.is_active = TRUE
      AND a.is_active = TRUE
      AND s.is_active = TRUE
    LIMIT 1;
$$;


ALTER FUNCTION public.fetch_auth_config_by_secret_hash(sk_hash_hex text) OWNER TO vaultless;

--
-- Name: get_bandwidth_quota_warnings(uuid, numeric); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.get_bandwidth_quota_warnings(p_user_id uuid, p_threshold numeric DEFAULT 80) RETURNS TABLE(application_id uuid, application_name character varying, bandwidth_quota_usage_percentage numeric)
    LANGUAGE sql STABLE
    AS $$
    SELECT 
        application_id,
        name AS application_name,
        bandwidth_quota_usage_percentage
    FROM mv_applications_with_usage
    WHERE developer_id = p_user_id
        AND bandwidth_quota_usage_percentage >= p_threshold
        AND is_active = true
    ORDER BY bandwidth_quota_usage_percentage DESC;
$$;


ALTER FUNCTION public.get_bandwidth_quota_warnings(p_user_id uuid, p_threshold numeric) OWNER TO vaultless;

--
-- Name: get_encrypted_group_key_for_client(uuid, uuid); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.get_encrypted_group_key_for_client(p_group_id uuid, p_client_id uuid) RETURNS jsonb
    LANGUAGE plpgsql STABLE
    AS $$
DECLARE
    v_keys JSONB;
    v_key JSONB;
BEGIN
    -- Get encrypted_group_keys from the group
    SELECT encrypted_group_keys INTO v_keys
    FROM message_groups
    WHERE id = p_group_id;

    -- If no keys found, return NULL
    IF v_keys IS NULL THEN
        RETURN NULL;
    END IF;

    -- Search for the client's key in the keys array
    FOR v_key IN SELECT * FROM jsonb_array_elements(v_keys->'keys')
    LOOP
        IF (v_key->>'client_id')::UUID = p_client_id THEN
            RETURN v_key;
        END IF;
    END LOOP;

    -- Key not found for this client
    RETURN NULL;
END;
$$;


ALTER FUNCTION public.get_encrypted_group_key_for_client(p_group_id uuid, p_client_id uuid) OWNER TO vaultless;

--
-- Name: FUNCTION get_encrypted_group_key_for_client(p_group_id uuid, p_client_id uuid); Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON FUNCTION public.get_encrypted_group_key_for_client(p_group_id uuid, p_client_id uuid) IS 'Returns the encrypted group key for a specific client in a group. Used by clients to decrypt group messages.';


--
-- Name: get_group_files_paginated(uuid, integer, integer); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.get_group_files_paginated(p_group_id uuid, p_limit integer DEFAULT 20, p_offset integer DEFAULT 0) RETURNS TABLE(file_id uuid, encrypted_filename text, file_size_bytes bigint, uploader_client_id uuid, created_at timestamp with time zone, download_count integer, total_count bigint)
    LANGUAGE plpgsql STABLE
    AS $$
BEGIN
    RETURN QUERY
    SELECT 
        gf.id,
        gf.encrypted_filename,
        gf.file_size_bytes,
        gf.uploader_client_id,
        gf.created_at,
        gf.download_count,
        COUNT(*) OVER() as total_count
    FROM group_files gf
    WHERE gf.group_id = p_group_id
        AND (gf.expires_at IS NULL OR gf.expires_at > NOW())
    ORDER BY gf.created_at DESC
    LIMIT p_limit
    OFFSET p_offset;
END;
$$;


ALTER FUNCTION public.get_group_files_paginated(p_group_id uuid, p_limit integer, p_offset integer) OWNER TO vaultless;

--
-- Name: get_group_member_addresses(uuid); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.get_group_member_addresses(p_group_id uuid) RETURNS uuid[]
    LANGUAGE plpgsql
    AS $$
BEGIN
    RETURN ARRAY(
        SELECT client_address
        FROM group_members
        WHERE group_id = p_group_id
            AND status = 'active'
    );
END;
$$;


ALTER FUNCTION public.get_group_member_addresses(p_group_id uuid) OWNER TO vaultless;

--
-- Name: get_monthly_revenue_chart_data(uuid, uuid, integer); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.get_monthly_revenue_chart_data(p_application_id uuid DEFAULT NULL::uuid, p_developer_id uuid DEFAULT NULL::uuid, p_months_back integer DEFAULT 12) RETURNS TABLE(month_label text, revenue_cents bigint, revenue_usd numeric, messages bigint, bytes_transferred bigint)
    LANGUAGE sql STABLE
    AS $_$
    WITH date_range AS (
        SELECT generate_series(
            date_trunc('month', now()) - ((p_months_back - 1) || ' months')::interval,
            date_trunc('month', now()),
            '1 month'::interval
        ) AS month
    ),
    revenue_data AS (
        SELECT 
            time_bucket('1 month', period_start) AS month,
            SUM(estimated_cost_cents) AS revenue_cents,
            SUM(messages_sent + messages_received) AS messages,
            SUM(total_bytes_sent + total_bytes_received) AS bytes_transferred
        FROM usage_metrics
        WHERE 
            ($1 IS NULL OR application_id = $1)  -- application filter
            AND ($2 IS NULL OR EXISTS (
                SELECT 1 FROM applications a 
                WHERE a.id = usage_metrics.application_id 
                AND a.developer_id = $2
            ))  -- developer filter
            AND period_start >= date_trunc('month', now()) - (($3 - 1) || ' months')::interval
        GROUP BY time_bucket('1 month', period_start)
    )
    SELECT 
        TO_CHAR(dr.month, 'YYYY-MM') AS month_label,
        COALESCE(rd.revenue_cents, 0) AS revenue_cents,
        (COALESCE(rd.revenue_cents, 0) / 100.0)::DECIMAL(10,2) AS revenue_usd,
        COALESCE(rd.messages, 0) AS messages,
        COALESCE(rd.bytes_transferred, 0) AS bytes_transferred
    FROM date_range dr
    LEFT JOIN revenue_data rd ON dr.month = rd.month
    ORDER BY dr.month;
$_$;


ALTER FUNCTION public.get_monthly_revenue_chart_data(p_application_id uuid, p_developer_id uuid, p_months_back integer) OWNER TO vaultless;

--
-- Name: get_or_create_client(character varying, text, uuid, jsonb); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.get_or_create_client(p_identifier_hash character varying, p_public_key text DEFAULT NULL::text, p_developer_id uuid DEFAULT NULL::uuid, p_metadata jsonb DEFAULT NULL::jsonb) RETURNS TABLE(client_id uuid, is_new boolean)
    LANGUAGE plpgsql
    AS $$
DECLARE
    v_client_id UUID;
    v_is_new BOOLEAN;
BEGIN
    -- Try to find existing client
    SELECT id INTO v_client_id
    FROM clients
    WHERE client_identifier_hash = p_identifier_hash;
    
    IF v_client_id IS NULL THEN
        -- Create new client
        INSERT INTO clients (
            client_identifier_hash,
            public_key,
            developer_id,
            metadata,
            last_seen_at
        )
        VALUES (
            p_identifier_hash,
            p_public_key,
            p_developer_id,
            p_metadata,
            NOW()
        )
        RETURNING id INTO v_client_id;
        
        v_is_new := TRUE;
    ELSE
        -- Update last_seen for existing client
        UPDATE clients 
        SET last_seen_at = NOW()
        WHERE id = v_client_id;
        
        v_is_new := FALSE;
    END IF;
    
    RETURN QUERY SELECT v_client_id, v_is_new;
END;
$$;


ALTER FUNCTION public.get_or_create_client(p_identifier_hash character varying, p_public_key text, p_developer_id uuid, p_metadata jsonb) OWNER TO vaultless;

--
-- Name: FUNCTION get_or_create_client(p_identifier_hash character varying, p_public_key text, p_developer_id uuid, p_metadata jsonb); Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON FUNCTION public.get_or_create_client(p_identifier_hash character varying, p_public_key text, p_developer_id uuid, p_metadata jsonb) IS 'Idempotent client registration. Returns existing client or creates new one.';


--
-- Name: get_reaction_summary(uuid, uuid); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.get_reaction_summary(p_message_id uuid, p_client_id uuid DEFAULT NULL::uuid) RETURNS TABLE(encrypted_reaction text, reaction_count bigint, reacted_by_me boolean)
    LANGUAGE plpgsql STABLE
    AS $$
BEGIN
    RETURN QUERY
    SELECT 
        mr.encrypted_reaction,
        COUNT(*)::BIGINT as reaction_count,
        BOOL_OR(mr.client_id = p_client_id) as reacted_by_me
    FROM message_reactions mr
    WHERE mr.message_id = p_message_id
    GROUP BY mr.encrypted_reaction
    ORDER BY reaction_count DESC;
END;
$$;


ALTER FUNCTION public.get_reaction_summary(p_message_id uuid, p_client_id uuid) OWNER TO vaultless;

--
-- Name: get_sender_key_for_recipient(uuid, uuid, uuid); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.get_sender_key_for_recipient(p_group_id uuid, p_sender_id uuid, p_recipient_id uuid) RETURNS TABLE(encrypted_chain_key text, key_version integer, signing_key text)
    LANGUAGE plpgsql STABLE
    AS $$
BEGIN
    RETURN QUERY
    SELECT 
        sk.encrypted_chain_key,
        sk.key_version,
        gm.sender_chain_public_key
    FROM sender_keys sk
    INNER JOIN group_members gm ON sk.sender_client_id = gm.client_address 
        AND sk.group_id = gm.group_id
    WHERE sk.group_id = p_group_id
        AND sk.sender_client_id = p_sender_id
        AND sk.recipient_client_id = p_recipient_id
        AND gm.status = 'active'
    ORDER BY sk.key_version DESC
    LIMIT 1;
END;
$$;


ALTER FUNCTION public.get_sender_key_for_recipient(p_group_id uuid, p_sender_id uuid, p_recipient_id uuid) OWNER TO vaultless;

--
-- Name: get_unread_notification_count(uuid); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.get_unread_notification_count(p_user_id uuid) RETURNS bigint
    LANGUAGE plpgsql
    AS $$
DECLARE
    unread_count BIGINT;
BEGIN
    SELECT COUNT(*) INTO unread_count
    FROM notifications
    WHERE user_id = p_user_id
        AND is_read = FALSE
        AND (expires_at IS NULL OR expires_at > NOW());
    
    RETURN unread_count;
END;
$$;


ALTER FUNCTION public.get_unread_notification_count(p_user_id uuid) OWNER TO vaultless;

--
-- Name: get_user_usage_summary(uuid); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.get_user_usage_summary(p_developer_id uuid) RETURNS TABLE(total_apps integer, total_monthly_messages bigint, total_clients bigint, total_monthly_cost bigint, critical_quota_apps integer, critical_bandwidth_quota_apps integer, total_monthly_revenue_cents bigint)
    LANGUAGE sql STABLE
    AS $$
    SELECT
        COUNT(*)::INTEGER,
        COALESCE(SUM(current_month_messages_sent), 0)::BIGINT,
        COALESCE(SUM(client_count), 0)::BIGINT,
        COALESCE(SUM(current_month_cost_cents), 0)::BIGINT,
        COUNT(*) FILTER (WHERE quota_usage_percentage >= 90)::INTEGER,
        COUNT(*) FILTER (WHERE bandwidth_quota_usage_percentage >= 90)::INTEGER,
        COALESCE(SUM(current_month_revenue_cents), 0)::BIGINT
    FROM mv_applications_with_usage
    WHERE developer_id = p_developer_id;
$$;


ALTER FUNCTION public.get_user_usage_summary(p_developer_id uuid) OWNER TO vaultless;

--
-- Name: is_group_member(uuid, uuid); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.is_group_member(p_group_id uuid, p_client_address uuid) RETURNS boolean
    LANGUAGE plpgsql
    AS $$
DECLARE
    v_exists BOOLEAN;
BEGIN
    SELECT EXISTS(
        SELECT 1 FROM group_members
        WHERE group_id = p_group_id
            AND client_address = p_client_address
            AND status = 'active'
    ) INTO v_exists;

    RETURN v_exists;
END;
$$;


ALTER FUNCTION public.is_group_member(p_group_id uuid, p_client_address uuid) OWNER TO vaultless;

--
-- Name: mark_group_message_read(uuid, uuid, uuid); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.mark_group_message_read(p_message_id uuid, p_group_id uuid, p_client_address uuid) RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
    INSERT INTO group_message_read_receipts (
        message_id,
        group_id,
        client_address,
        read_at
    )
    VALUES (p_message_id, p_group_id, p_client_address, NOW())
    ON CONFLICT (message_id, client_address) DO NOTHING;

    UPDATE group_members
    SET unread_count = GREATEST(unread_count - 1, 0),
        last_read_at = NOW()
    WHERE group_id = p_group_id
        AND client_address = p_client_address;
END;
$$;


ALTER FUNCTION public.mark_group_message_read(p_message_id uuid, p_group_id uuid, p_client_address uuid) OWNER TO vaultless;

--
-- Name: revoke_refresh_token_family(uuid); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.revoke_refresh_token_family(p_token_family uuid) RETURNS integer
    LANGUAGE plpgsql
    AS $$
DECLARE
    updated_count INTEGER;
BEGIN
    UPDATE refresh_tokens 
    SET 
        is_revoked = true, 
        revoked_at = NOW(),
        revoked_reason = 'Token family compromised - possible theft detected'
    WHERE token_family = p_token_family 
        AND is_revoked = false;
    
    GET DIAGNOSTICS updated_count = ROW_COUNT;
    RETURN updated_count;
END;
$$;


ALTER FUNCTION public.revoke_refresh_token_family(p_token_family uuid) OWNER TO vaultless;

--
-- Name: revoke_user_sessions(uuid); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.revoke_user_sessions(p_user_id uuid) RETURNS integer
    LANGUAGE plpgsql
    AS $$
DECLARE
    updated_count INTEGER;
BEGIN
    UPDATE user_sessions 
    SET is_active = false, revoked_at = NOW() 
    WHERE user_id = p_user_id AND is_active = true;
    
    GET DIAGNOSTICS updated_count = ROW_COUNT;
    RETURN updated_count;
END;
$$;


ALTER FUNCTION public.revoke_user_sessions(p_user_id uuid) OWNER TO vaultless;

--
-- Name: rotate_group_encryption_key(uuid, jsonb); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.rotate_group_encryption_key(p_group_id uuid, p_new_encrypted_keys jsonb) RETURNS integer
    LANGUAGE plpgsql
    AS $$
DECLARE
    v_new_version INTEGER;
BEGIN
    -- Increment key version and update encrypted keys
    UPDATE message_groups
    SET 
        key_version = key_version + 1,
        encrypted_group_keys = p_new_encrypted_keys,
        updated_at = NOW()
    WHERE id = p_group_id
    RETURNING key_version INTO v_new_version;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'Group not found: %', p_group_id;
    END IF;

    RETURN v_new_version;
END;
$$;


ALTER FUNCTION public.rotate_group_encryption_key(p_group_id uuid, p_new_encrypted_keys jsonb) OWNER TO vaultless;

--
-- Name: FUNCTION rotate_group_encryption_key(p_group_id uuid, p_new_encrypted_keys jsonb); Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON FUNCTION public.rotate_group_encryption_key(p_group_id uuid, p_new_encrypted_keys jsonb) IS 'Rotates the group encryption key by incrementing version and updating all member keys. Should be called after member removal.';


--
-- Name: set_notification_read_at(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.set_notification_read_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.is_read = TRUE AND OLD.is_read = FALSE THEN
        NEW.read_at = NOW();
    END IF;
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.set_notification_read_at() OWNER TO vaultless;

--
-- Name: update_application_last_message(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.update_application_last_message() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    UPDATE public.applications
    SET updated_at = NOW()
    WHERE id = NEW.application_id;
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.update_application_last_message() OWNER TO vaultless;

--
-- Name: update_applications_updated_at(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.update_applications_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.update_applications_updated_at() OWNER TO vaultless;

--
-- Name: update_client_last_message(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.update_client_last_message() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- Update both sender and recipient without revealing correlation
    IF NEW.sender_client_id IS NOT NULL THEN
        UPDATE clients 
        SET last_message_at = NEW.created_at
        WHERE id = NEW.sender_client_id;
    END IF;
    
    IF NEW.recipient_client_id IS NOT NULL THEN
        UPDATE clients 
        SET last_message_at = NEW.created_at
        WHERE id = NEW.recipient_client_id;
    END IF;
    
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.update_client_last_message() OWNER TO vaultless;

--
-- Name: update_clients_updated_at(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.update_clients_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.update_clients_updated_at() OWNER TO vaultless;

--
-- Name: update_group_member_count(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.update_group_member_count() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.status = 'active' THEN
            UPDATE message_groups
            SET member_count = member_count + 1,
                updated_at = NOW()
            WHERE id = NEW.group_id;
        END IF;
    ELSIF TG_OP = 'UPDATE' THEN
        IF OLD.status = 'active' AND NEW.status != 'active' THEN
            UPDATE message_groups
            SET member_count = GREATEST(member_count - 1, 0),
                updated_at = NOW()
            WHERE id = NEW.group_id;
        ELSIF OLD.status != 'active' AND NEW.status = 'active' THEN
            UPDATE message_groups
            SET member_count = member_count + 1,
                updated_at = NOW()
            WHERE id = NEW.group_id;
        END IF;
    END IF;
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.update_group_member_count() OWNER TO vaultless;

--
-- Name: update_group_message_stats(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.update_group_message_stats() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.group_id IS NOT NULL THEN
        UPDATE message_groups
        SET
            last_message_at = GREATEST(NEW.created_at, COALESCE(last_message_at, to_timestamp(0))),
            message_count = message_count + 1,
            updated_at = NOW()
        WHERE id = NEW.group_id;

        UPDATE group_members
        SET unread_count = unread_count + 1
        WHERE group_id = NEW.group_id
            AND client_address != NEW.sender_client_address
            AND status = 'active';
    END IF;
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.update_group_message_stats() OWNER TO vaultless;

--
-- Name: update_notifications_updated_at(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.update_notifications_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.update_notifications_updated_at() OWNER TO vaultless;

--
-- Name: update_updated_at(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.update_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.update_updated_at() OWNER TO vaultless;

--
-- Name: update_updated_at_column(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.update_updated_at_column() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.update_updated_at_column() OWNER TO vaultless;

--
-- Name: validate_encrypted_keys_structure(); Type: FUNCTION; Schema: public; Owner: vaultless
--

CREATE FUNCTION public.validate_encrypted_keys_structure() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.encrypted_group_keys IS NOT NULL THEN
        -- Check that it has a 'keys' array
        IF NOT (NEW.encrypted_group_keys ? 'keys') THEN
            RAISE EXCEPTION 'encrypted_group_keys must contain a "keys" array';
        END IF;
        
        -- Check that 'keys' is actually an array
        IF jsonb_typeof(NEW.encrypted_group_keys->'keys') != 'array' THEN
            RAISE EXCEPTION 'encrypted_group_keys["keys"] must be an array';
        END IF;
    END IF;
    
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.validate_encrypted_keys_structure() OWNER TO vaultless;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: _compressed_hypertable_2; Type: TABLE; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE TABLE _timescaledb_internal._compressed_hypertable_2 (
);


ALTER TABLE _timescaledb_internal._compressed_hypertable_2 OWNER TO vaultless;

--
-- Name: usage_metrics; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.usage_metrics (
    period_start timestamp with time zone NOT NULL,
    period_end timestamp with time zone NOT NULL,
    application_id uuid NOT NULL,
    subscription_id uuid NOT NULL,
    api_key_id uuid,
    messages_sent bigint DEFAULT 0 NOT NULL,
    messages_received bigint DEFAULT 0 NOT NULL,
    proofs_verified bigint DEFAULT 0 NOT NULL,
    total_bytes_stored bigint DEFAULT 0 NOT NULL,
    total_bytes_sent bigint DEFAULT 0 NOT NULL,
    total_bytes_received bigint DEFAULT 0 NOT NULL,
    rate_limit_hits integer DEFAULT 0 NOT NULL,
    estimated_cost_cents bigint DEFAULT 0,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT valid_counters CHECK (((messages_sent >= 0) AND (messages_received >= 0) AND (proofs_verified >= 0) AND (total_bytes_stored >= 0) AND (total_bytes_sent >= 0) AND (total_bytes_received >= 0) AND (rate_limit_hits >= 0))),
    CONSTRAINT valid_period CHECK ((period_end > period_start))
);


ALTER TABLE public.usage_metrics OWNER TO vaultless;

--
-- Name: _direct_view_3; Type: VIEW; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE VIEW _timescaledb_internal._direct_view_3 AS
 SELECT application_id,
    subscription_id,
    public.time_bucket('1 day'::interval, period_start) AS day,
    (sum(messages_sent))::bigint AS total_messages_sent,
    (sum(messages_received))::bigint AS total_messages_received,
    (sum(proofs_verified))::bigint AS total_proofs_verified,
    (sum(total_bytes_stored))::bigint AS total_bytes_stored,
    (sum(total_bytes_sent))::bigint AS total_bytes_sent,
    (sum(total_bytes_received))::bigint AS total_bytes_received,
    sum(rate_limit_hits) AS total_rate_limit_hits,
    (sum(estimated_cost_cents))::bigint AS total_estimated_cost_cents
   FROM public.usage_metrics
  GROUP BY application_id, subscription_id, (public.time_bucket('1 day'::interval, period_start));


ALTER VIEW _timescaledb_internal._direct_view_3 OWNER TO vaultless;

--
-- Name: _direct_view_4; Type: VIEW; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE VIEW _timescaledb_internal._direct_view_4 AS
 SELECT application_id,
    api_key_id,
    public.time_bucket('1 day'::interval, period_start) AS day,
    (sum(messages_sent))::bigint AS total_messages_sent,
    sum(rate_limit_hits) AS total_rate_limit_hits
   FROM public.usage_metrics
  WHERE (api_key_id IS NOT NULL)
  GROUP BY application_id, api_key_id, (public.time_bucket('1 day'::interval, period_start));


ALTER VIEW _timescaledb_internal._direct_view_4 OWNER TO vaultless;

--
-- Name: client_usage_metrics; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.client_usage_metrics (
    period_start timestamp with time zone NOT NULL,
    period_end timestamp with time zone NOT NULL,
    application_id uuid NOT NULL,
    client_id uuid NOT NULL,
    messages_sent bigint DEFAULT 0 NOT NULL,
    messages_received bigint DEFAULT 0 NOT NULL,
    proofs_verified bigint DEFAULT 0 NOT NULL,
    total_bytes_stored bigint DEFAULT 0 NOT NULL,
    total_bytes_sent bigint DEFAULT 0 NOT NULL,
    total_bytes_received bigint DEFAULT 0 NOT NULL,
    rate_limit_hits integer DEFAULT 0 NOT NULL,
    estimated_cost_cents bigint DEFAULT 0 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT client_usage_valid_counters CHECK (((messages_sent >= 0) AND (messages_received >= 0) AND (proofs_verified >= 0) AND (total_bytes_stored >= 0) AND (total_bytes_sent >= 0) AND (total_bytes_received >= 0) AND (rate_limit_hits >= 0) AND (estimated_cost_cents >= 0))),
    CONSTRAINT client_usage_valid_period CHECK ((period_end > period_start))
);


ALTER TABLE public.client_usage_metrics OWNER TO vaultless;

--
-- Name: _direct_view_6; Type: VIEW; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE VIEW _timescaledb_internal._direct_view_6 AS
 SELECT application_id,
    client_id,
    public.time_bucket('30 days'::interval, period_start) AS period_start,
    max(period_end) AS period_end,
    sum(messages_sent) AS messages_sent,
    sum(messages_received) AS messages_received,
    sum(proofs_verified) AS proofs_verified,
    sum(total_bytes_stored) AS total_bytes_stored,
    sum(total_bytes_sent) AS total_bytes_sent,
    sum(total_bytes_received) AS total_bytes_received,
    sum(rate_limit_hits) AS rate_limit_hits,
    sum(estimated_cost_cents) AS estimated_cost_cents
   FROM public.client_usage_metrics
  GROUP BY application_id, client_id, (public.time_bucket('30 days'::interval, period_start));


ALTER VIEW _timescaledb_internal._direct_view_6 OWNER TO vaultless;

--
-- Name: _direct_view_7; Type: VIEW; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE VIEW _timescaledb_internal._direct_view_7 AS
 SELECT application_id,
    public.time_bucket('1 mon'::interval, period_start) AS month,
    sum(estimated_cost_cents) AS total_revenue_cents,
    sum((messages_sent + messages_received)) AS total_messages,
    sum((total_bytes_sent + total_bytes_received)) AS total_bytes,
    sum(proofs_verified) AS total_proofs,
    count(*) AS billing_records_count,
    avg(estimated_cost_cents) AS avg_cost_per_record,
    min(estimated_cost_cents) AS min_cost_record,
    max(estimated_cost_cents) AS max_cost_record
   FROM public.usage_metrics
  GROUP BY application_id, (public.time_bucket('1 mon'::interval, period_start));


ALTER VIEW _timescaledb_internal._direct_view_7 OWNER TO vaultless;

--
-- Name: applications; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.applications (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    developer_id uuid NOT NULL,
    subscription_id uuid,
    name character varying(255) NOT NULL,
    description text,
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    max_ttl_seconds integer DEFAULT 604800 NOT NULL,
    is_key_rotation_forced boolean DEFAULT false NOT NULL,
    deletion_requested_at timestamp with time zone,
    internal_notes text,
    app_meta jsonb DEFAULT '{"IntegrityConfig": {"ios": {"apple_team_id": "ABCD123456", "min_version_code": 100, "allowed_bundle_ids": ["com.example.app"], "reject_untrusted_device": true, "allowed_certificate_hashes": []}, "iot": {"require_cn_match": true, "reject_future_certificates": true, "allowed_certificate_authorities": ["Example Root CA"], "require_valid_certificate_expiry": true}, "android": {"google_api_key": null, "min_version_code": 100, "google_cloud_project": "project-123", "allowed_package_names": ["com.example.app"], "max_token_age_seconds": 60, "reject_untrusted_device": true, "allowed_certificate_sha256": [], "reject_unrecognized_version": true}, "browser": {"captcha_provider": "turnstile", "captcha_site_key": null, "cors_strict_mode": true, "authorized_origins": ["https://app.example.com"], "captcha_secret_key": null, "max_clients_per_ip": 5, "alert_on_usage_spike": true, "track_origin_changes": true, "usage_baseline_hours": 24, "bind_client_to_origin": true, "require_origin_header": true, "usage_spike_threshold": 2.0, "require_referer_header": true, "max_requests_per_ip_per_hour": 1000, "max_origin_changes_per_client": 3, "require_captcha_on_registration": true, "max_registrations_per_ip_per_hour": 10}, "rate_limits": {"max_attestations_per_user_per_hour": 100, "max_failed_attempts_before_lockout": 5}, "allow_unauthenticated": false}, "PlatformFingerPrint": {"ios": "00000000-0000-0000-0000-000000000000", "iot": "00000000-0000-0000-0000-000000000000", "android": "00000000-0000-0000-0000-000000000000", "browser": "00000000-0000-0000-0000-000000000000"}}'::jsonb NOT NULL,
    CONSTRAINT valid_name CHECK ((char_length((name)::text) > 0))
);


ALTER TABLE public.applications OWNER TO vaultless;

--
-- Name: _direct_view_8; Type: VIEW; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE VIEW _timescaledb_internal._direct_view_8 AS
 SELECT a.developer_id,
    public.time_bucket('1 mon'::interval, um.period_start) AS month,
    sum(um.estimated_cost_cents) AS total_revenue_cents,
    count(DISTINCT um.application_id) AS applications_count,
    count(DISTINCT um.api_key_id) AS api_keys_count,
    sum((um.messages_sent + um.messages_received)) AS total_messages,
    sum((um.total_bytes_sent + um.total_bytes_received)) AS total_bytes
   FROM (public.usage_metrics um
     JOIN public.applications a ON ((um.application_id = a.id)))
  GROUP BY a.developer_id, (public.time_bucket('1 mon'::interval, um.period_start));


ALTER VIEW _timescaledb_internal._direct_view_8 OWNER TO vaultless;

--
-- Name: _materialized_hypertable_3; Type: TABLE; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE TABLE _timescaledb_internal._materialized_hypertable_3 (
    application_id uuid,
    subscription_id uuid,
    day timestamp with time zone NOT NULL,
    total_messages_sent bigint,
    total_messages_received bigint,
    total_proofs_verified bigint,
    total_bytes_stored bigint,
    total_bytes_sent bigint,
    total_bytes_received bigint,
    total_rate_limit_hits bigint,
    total_estimated_cost_cents bigint
);


ALTER TABLE _timescaledb_internal._materialized_hypertable_3 OWNER TO vaultless;

--
-- Name: _materialized_hypertable_4; Type: TABLE; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE TABLE _timescaledb_internal._materialized_hypertable_4 (
    application_id uuid,
    api_key_id uuid,
    day timestamp with time zone NOT NULL,
    total_messages_sent bigint,
    total_rate_limit_hits bigint
);


ALTER TABLE _timescaledb_internal._materialized_hypertable_4 OWNER TO vaultless;

--
-- Name: _materialized_hypertable_6; Type: TABLE; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE TABLE _timescaledb_internal._materialized_hypertable_6 (
    application_id uuid,
    client_id uuid,
    period_start timestamp with time zone NOT NULL,
    period_end timestamp with time zone,
    messages_sent numeric,
    messages_received numeric,
    proofs_verified numeric,
    total_bytes_stored numeric,
    total_bytes_sent numeric,
    total_bytes_received numeric,
    rate_limit_hits bigint,
    estimated_cost_cents numeric
);


ALTER TABLE _timescaledb_internal._materialized_hypertable_6 OWNER TO vaultless;

--
-- Name: _materialized_hypertable_7; Type: TABLE; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE TABLE _timescaledb_internal._materialized_hypertable_7 (
    application_id uuid,
    month timestamp with time zone NOT NULL,
    total_revenue_cents numeric,
    total_messages numeric,
    total_bytes numeric,
    total_proofs numeric,
    billing_records_count bigint,
    avg_cost_per_record numeric,
    min_cost_record bigint,
    max_cost_record bigint
);


ALTER TABLE _timescaledb_internal._materialized_hypertable_7 OWNER TO vaultless;

--
-- Name: _materialized_hypertable_8; Type: TABLE; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE TABLE _timescaledb_internal._materialized_hypertable_8 (
    developer_id uuid,
    month timestamp with time zone NOT NULL,
    total_revenue_cents numeric,
    applications_count bigint,
    api_keys_count bigint,
    total_messages numeric,
    total_bytes numeric
);


ALTER TABLE _timescaledb_internal._materialized_hypertable_8 OWNER TO vaultless;

--
-- Name: _partial_view_3; Type: VIEW; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE VIEW _timescaledb_internal._partial_view_3 AS
 SELECT application_id,
    subscription_id,
    public.time_bucket('1 day'::interval, period_start) AS day,
    (sum(messages_sent))::bigint AS total_messages_sent,
    (sum(messages_received))::bigint AS total_messages_received,
    (sum(proofs_verified))::bigint AS total_proofs_verified,
    (sum(total_bytes_stored))::bigint AS total_bytes_stored,
    (sum(total_bytes_sent))::bigint AS total_bytes_sent,
    (sum(total_bytes_received))::bigint AS total_bytes_received,
    sum(rate_limit_hits) AS total_rate_limit_hits,
    (sum(estimated_cost_cents))::bigint AS total_estimated_cost_cents
   FROM public.usage_metrics
  GROUP BY application_id, subscription_id, (public.time_bucket('1 day'::interval, period_start));


ALTER VIEW _timescaledb_internal._partial_view_3 OWNER TO vaultless;

--
-- Name: _partial_view_4; Type: VIEW; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE VIEW _timescaledb_internal._partial_view_4 AS
 SELECT application_id,
    api_key_id,
    public.time_bucket('1 day'::interval, period_start) AS day,
    (sum(messages_sent))::bigint AS total_messages_sent,
    sum(rate_limit_hits) AS total_rate_limit_hits
   FROM public.usage_metrics
  WHERE (api_key_id IS NOT NULL)
  GROUP BY application_id, api_key_id, (public.time_bucket('1 day'::interval, period_start));


ALTER VIEW _timescaledb_internal._partial_view_4 OWNER TO vaultless;

--
-- Name: _partial_view_6; Type: VIEW; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE VIEW _timescaledb_internal._partial_view_6 AS
 SELECT application_id,
    client_id,
    public.time_bucket('30 days'::interval, period_start) AS period_start,
    max(period_end) AS period_end,
    sum(messages_sent) AS messages_sent,
    sum(messages_received) AS messages_received,
    sum(proofs_verified) AS proofs_verified,
    sum(total_bytes_stored) AS total_bytes_stored,
    sum(total_bytes_sent) AS total_bytes_sent,
    sum(total_bytes_received) AS total_bytes_received,
    sum(rate_limit_hits) AS rate_limit_hits,
    sum(estimated_cost_cents) AS estimated_cost_cents
   FROM public.client_usage_metrics
  GROUP BY application_id, client_id, (public.time_bucket('30 days'::interval, period_start));


ALTER VIEW _timescaledb_internal._partial_view_6 OWNER TO vaultless;

--
-- Name: _partial_view_7; Type: VIEW; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE VIEW _timescaledb_internal._partial_view_7 AS
 SELECT application_id,
    public.time_bucket('1 mon'::interval, period_start) AS month,
    sum(estimated_cost_cents) AS total_revenue_cents,
    sum((messages_sent + messages_received)) AS total_messages,
    sum((total_bytes_sent + total_bytes_received)) AS total_bytes,
    sum(proofs_verified) AS total_proofs,
    count(*) AS billing_records_count,
    avg(estimated_cost_cents) AS avg_cost_per_record,
    min(estimated_cost_cents) AS min_cost_record,
    max(estimated_cost_cents) AS max_cost_record
   FROM public.usage_metrics
  GROUP BY application_id, (public.time_bucket('1 mon'::interval, period_start));


ALTER VIEW _timescaledb_internal._partial_view_7 OWNER TO vaultless;

--
-- Name: _partial_view_8; Type: VIEW; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE VIEW _timescaledb_internal._partial_view_8 AS
 SELECT a.developer_id,
    public.time_bucket('1 mon'::interval, um.period_start) AS month,
    sum(um.estimated_cost_cents) AS total_revenue_cents,
    count(DISTINCT um.application_id) AS applications_count,
    count(DISTINCT um.api_key_id) AS api_keys_count,
    sum((um.messages_sent + um.messages_received)) AS total_messages,
    sum((um.total_bytes_sent + um.total_bytes_received)) AS total_bytes
   FROM (public.usage_metrics um
     JOIN public.applications a ON ((um.application_id = a.id)))
  GROUP BY a.developer_id, (public.time_bucket('1 mon'::interval, um.period_start));


ALTER VIEW _timescaledb_internal._partial_view_8 OWNER TO vaultless;

--
-- Name: _sqlx_migrations; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public._sqlx_migrations (
    version bigint NOT NULL,
    description text NOT NULL,
    installed_on timestamp with time zone DEFAULT now() NOT NULL,
    success boolean NOT NULL,
    checksum bytea NOT NULL,
    execution_time bigint NOT NULL
);


ALTER TABLE public._sqlx_migrations OWNER TO vaultless;

--
-- Name: clients; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.clients (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    identifier character varying(64) DEFAULT NULL::character varying,
    client_identifier_hash character varying(64),
    last_jti character varying(36),
    allow_anonymous_messages boolean DEFAULT true NOT NULL,
    require_proof_verification boolean DEFAULT false NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    last_seen_at timestamp with time zone,
    last_message_at timestamp with time zone,
    developer_id uuid NOT NULL,
    metadata jsonb,
    application_id uuid NOT NULL,
    is_platform_attested boolean DEFAULT false NOT NULL,
    signing_key text NOT NULL
);


ALTER TABLE public.clients OWNER TO vaultless;

--
-- Name: TABLE clients; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.clients IS 'Anonymous ephemeral identity table - Zero personal information stored. 
Only cryptographic hashes and optional public keys. True privacy-first design.';


--
-- Name: COLUMN clients.client_identifier_hash; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.clients.client_identifier_hash IS 'SHA-256 hash of client identifier (public key or device fingerprint). 
MUST be computed CLIENT-SIDE. Server never sees plaintext.';


--
-- Name: COLUMN clients.last_seen_at; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.clients.last_seen_at IS 'Privacy-preserving activity tracking. No correlation with identity.';


--
-- Name: COLUMN clients.metadata; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.clients.metadata IS 'Encrypted metadata storage. NEVER store PII. 
Safe: device type, app version, locale, preferences. 
Forbidden: names, emails, phone numbers, addresses.';


--
-- Name: COLUMN clients.application_id; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.clients.application_id IS 'Links client to the application they registered through. Enables per-app analytics and client management.';


--
-- Name: COLUMN clients.signing_key; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.clients.signing_key IS 'Ed25519 public key for signature verification (authentication)';


--
-- Name: active_clients_summary; Type: VIEW; Schema: public; Owner: vaultless
--

CREATE VIEW public.active_clients_summary AS
 SELECT date_trunc('day'::text, created_at) AS registration_date,
    count(*) AS total_registrations,
    count(*) FILTER (WHERE (last_seen_at > (now() - '1 day'::interval))) AS active_1d,
    count(*) FILTER (WHERE (last_seen_at > (now() - '7 days'::interval))) AS active_7d,
    count(*) FILTER (WHERE (last_seen_at > (now() - '30 days'::interval))) AS active_30d,
    developer_id
   FROM public.clients
  WHERE (is_active = true)
  GROUP BY (date_trunc('day'::text, created_at)), developer_id;


ALTER VIEW public.active_clients_summary OWNER TO vaultless;

--
-- Name: api_keys; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.api_keys (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    user_id uuid,
    key_hash character varying(64),
    key_prefix character varying(32) NOT NULL,
    description text,
    scopes text,
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone,
    last_used_at timestamp with time zone,
    application_id uuid,
    key_type public.key_type DEFAULT 'secret'::public.key_type NOT NULL,
    publishable_key_plaintext character varying(64),
    CONSTRAINT required_key_data_check CHECK ((((key_prefix IS NOT NULL) AND ((key_type = 'secret'::public.key_type) AND (key_hash IS NOT NULL) AND (publishable_key_plaintext IS NULL))) OR ((key_type = 'publishable'::public.key_type) AND (publishable_key_plaintext IS NOT NULL) AND (key_hash IS NULL))))
);


ALTER TABLE public.api_keys OWNER TO vaultless;

--
-- Name: TABLE api_keys; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.api_keys IS 'Authentication keys with subscription tier and quota management';


--
-- Name: application_pricing_plans; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.application_pricing_plans (
    application_id uuid NOT NULL,
    pricing_plan_id uuid NOT NULL,
    is_default boolean DEFAULT false NOT NULL,
    attached_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.application_pricing_plans OWNER TO vaultless;

--
-- Name: billing_periods; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.billing_periods (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    application_id uuid NOT NULL,
    developer_id uuid NOT NULL,
    period_start timestamp with time zone NOT NULL,
    period_end timestamp with time zone NOT NULL,
    status text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT billing_period_valid CHECK ((period_end > period_start)),
    CONSTRAINT billing_periods_status_check CHECK ((status = ANY (ARRAY['open'::text, 'closed'::text, 'invoiced'::text])))
);


ALTER TABLE public.billing_periods OWNER TO vaultless;

--
-- Name: client_billing_usage; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.client_billing_usage (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    billing_period_id uuid NOT NULL,
    client_id uuid NOT NULL,
    application_id uuid NOT NULL,
    messages_sent bigint NOT NULL,
    messages_received bigint NOT NULL,
    proofs_verified bigint NOT NULL,
    total_bytes_stored bigint NOT NULL,
    total_bytes_sent bigint NOT NULL,
    total_bytes_received bigint NOT NULL,
    rate_limit_hits integer NOT NULL,
    developer_id uuid NOT NULL,
    revenue_snapshot jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.client_billing_usage OWNER TO vaultless;

--
-- Name: client_invoices; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.client_invoices (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    billing_period_id uuid NOT NULL,
    client_id uuid NOT NULL,
    application_id uuid NOT NULL,
    developer_id uuid NOT NULL,
    pricing_snapshot jsonb NOT NULL,
    subtotal_cents bigint NOT NULL,
    total_cents bigint NOT NULL,
    status text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT client_invoices_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'finalized'::text, 'paid'::text, 'failed'::text])))
);


ALTER TABLE public.client_invoices OWNER TO vaultless;

--
-- Name: client_subscriptions; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.client_subscriptions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    client_id uuid NOT NULL,
    application_id uuid NOT NULL,
    pricing_plan_id uuid NOT NULL,
    status public.subscription_status_enum DEFAULT 'active'::public.subscription_status_enum NOT NULL,
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    ended_at timestamp with time zone,
    pricing_snapshot jsonb NOT NULL
);


ALTER TABLE public.client_subscriptions OWNER TO vaultless;

--
-- Name: client_usage_monthly; Type: VIEW; Schema: public; Owner: vaultless
--

CREATE VIEW public.client_usage_monthly AS
 SELECT application_id,
    client_id,
    period_start,
    period_end,
    messages_sent,
    messages_received,
    proofs_verified,
    total_bytes_stored,
    total_bytes_sent,
    total_bytes_received,
    rate_limit_hits,
    estimated_cost_cents
   FROM _timescaledb_internal._materialized_hypertable_6;


ALTER VIEW public.client_usage_monthly OWNER TO vaultless;

--
-- Name: developer_subscriptions; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.developer_subscriptions (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    developer_id uuid NOT NULL,
    tier public.subscription_tier DEFAULT 'free'::public.subscription_tier NOT NULL,
    monthly_message_quota bigint DEFAULT 1000 NOT NULL,
    message_retention_seconds bigint DEFAULT 604800 NOT NULL,
    rate_limit_per_minute integer DEFAULT 60 NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    current_period_start timestamp with time zone DEFAULT now() NOT NULL,
    current_period_end timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    monthly_bandwidth_quota bigint DEFAULT 1073741824 NOT NULL
);


ALTER TABLE public.developer_subscriptions OWNER TO vaultless;

--
-- Name: TABLE developer_subscriptions; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.developer_subscriptions IS 'Developer subscription plans with message and bandwidth quotas';


--
-- Name: COLUMN developer_subscriptions.monthly_bandwidth_quota; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.developer_subscriptions.monthly_bandwidth_quota IS 'Monthly bandwidth quota in bytes. Used to limit data transfer for the subscription tier.';


--
-- Name: file_chunks; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.file_chunks (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    file_id uuid NOT NULL,
    chunk_index integer NOT NULL,
    encrypted_data bytea NOT NULL,
    chunk_size_bytes integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT valid_chunk_index CHECK ((chunk_index >= 0)),
    CONSTRAINT valid_chunk_size CHECK ((chunk_size_bytes > 0))
);


ALTER TABLE public.file_chunks OWNER TO vaultless;

--
-- Name: TABLE file_chunks; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.file_chunks IS 'Stores encrypted chunks for large files (> 10MB). Allows streaming and partial downloads.';


--
-- Name: group_activity_summary; Type: VIEW; Schema: public; Owner: vaultless
--

CREATE VIEW public.group_activity_summary AS
SELECT
    NULL::uuid AS group_id,
    NULL::character varying(255) AS group_name,
    NULL::integer AS member_count,
    NULL::integer AS message_count,
    NULL::bigint AS file_count,
    NULL::numeric AS total_file_size_bytes,
    NULL::bigint AS total_reactions,
    NULL::timestamp with time zone AS last_message_at,
    NULL::timestamp with time zone AS created_at;


ALTER VIEW public.group_activity_summary OWNER TO vaultless;

--
-- Name: group_files; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.group_files (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    group_id uuid NOT NULL,
    message_id uuid,
    uploader_client_id uuid NOT NULL,
    encrypted_filename text NOT NULL,
    encrypted_mime_type text NOT NULL,
    file_size_bytes bigint NOT NULL,
    encrypted_file_key text NOT NULL,
    nonce character varying(32) NOT NULL,
    storage_path text NOT NULL,
    chunk_count integer DEFAULT 1 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone,
    download_count integer DEFAULT 0 NOT NULL,
    max_downloads integer,
    CONSTRAINT valid_chunk_count CHECK ((chunk_count > 0)),
    CONSTRAINT valid_download_count CHECK ((download_count >= 0)),
    CONSTRAINT valid_file_size CHECK ((file_size_bytes > 0)),
    CONSTRAINT valid_max_downloads CHECK (((max_downloads IS NULL) OR (max_downloads > 0)))
);


ALTER TABLE public.group_files OWNER TO vaultless;

--
-- Name: TABLE group_files; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.group_files IS 'Encrypted files shared in groups. Files use separate encryption keys for performance.';


--
-- Name: group_members; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.group_members (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    group_id uuid NOT NULL,
    client_address uuid NOT NULL,
    role public.member_role_enum DEFAULT 'member'::public.member_role_enum NOT NULL,
    status public.member_status_enum DEFAULT 'active'::public.member_status_enum NOT NULL,
    can_send_messages boolean DEFAULT true NOT NULL,
    can_add_members boolean DEFAULT false NOT NULL,
    can_remove_members boolean DEFAULT false NOT NULL,
    joined_at timestamp with time zone DEFAULT now() NOT NULL,
    left_at timestamp with time zone,
    last_read_at timestamp with time zone,
    unread_count integer DEFAULT 0 NOT NULL,
    invited_by_client_address uuid,
    metadata jsonb,
    sender_chain_public_key text,
    sender_key_version integer DEFAULT 1 NOT NULL,
    CONSTRAINT chk_invite_self CHECK (((invited_by_client_address IS NULL) OR (invited_by_client_address <> client_address))),
    CONSTRAINT group_members_unread_count_check CHECK ((unread_count >= 0))
);


ALTER TABLE public.group_members OWNER TO vaultless;

--
-- Name: TABLE group_members; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.group_members IS 'Group membership with roles and permissions';


--
-- Name: COLUMN group_members.unread_count; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.group_members.unread_count IS 'Number of unread messages in this group';


--
-- Name: COLUMN group_members.sender_chain_public_key; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.group_members.sender_chain_public_key IS 'Public signing key for Sender Keys Protocol. Each sender has their own chain key.';


--
-- Name: message_groups; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.message_groups (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    group_name character varying(255),
    group_type public.group_type_enum DEFAULT 'private'::public.group_type_enum NOT NULL,
    creator_client_address uuid NOT NULL,
    allow_member_invite boolean DEFAULT false NOT NULL,
    require_admin_approval boolean DEFAULT true NOT NULL,
    max_members integer DEFAULT 100 NOT NULL,
    group_public_key text,
    is_active boolean DEFAULT true NOT NULL,
    is_archived boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    last_message_at timestamp with time zone,
    member_count integer DEFAULT 1 NOT NULL,
    message_count integer DEFAULT 0 NOT NULL,
    metadata jsonb,
    encrypted_group_keys jsonb,
    key_version integer DEFAULT 1 NOT NULL,
    uses_sender_keys boolean DEFAULT false NOT NULL,
    CONSTRAINT chk_positive_key_version CHECK ((key_version > 0)),
    CONSTRAINT message_groups_member_count_check CHECK ((member_count >= 0)),
    CONSTRAINT message_groups_message_count_check CHECK ((message_count >= 0))
);


ALTER TABLE public.message_groups OWNER TO vaultless;

--
-- Name: TABLE message_groups; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.message_groups IS 'Group chats and broadcast channels';


--
-- Name: COLUMN message_groups.encrypted_group_keys; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.message_groups.encrypted_group_keys IS 'JSON structure: {"keys": [{"client_id": "uuid", "encrypted_key": "base64", "key_version": 1, "encrypted_at": "timestamp"}]}';


--
-- Name: COLUMN message_groups.key_version; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.message_groups.key_version IS 'Incremented each time group key is rotated. Used for forward secrecy.';


--
-- Name: COLUMN message_groups.uses_sender_keys; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.message_groups.uses_sender_keys IS 'True: use Sender Keys Protocol (efficient for large groups). False: use shared group key';


--
-- Name: group_key_audit; Type: VIEW; Schema: public; Owner: vaultless
--

CREATE VIEW public.group_key_audit AS
 SELECT g.id AS group_id,
    g.group_name,
    g.key_version,
    g.created_at AS group_created_at,
    g.updated_at AS last_key_update,
    g.member_count,
    count(DISTINCT m.id) FILTER (WHERE (m.status = 'active'::public.member_status_enum)) AS active_members,
        CASE
            WHEN (g.encrypted_group_keys IS NOT NULL) THEN jsonb_array_length((g.encrypted_group_keys -> 'keys'::text))
            ELSE 0
        END AS encrypted_keys_count
   FROM (public.message_groups g
     LEFT JOIN public.group_members m ON ((g.id = m.group_id)))
  WHERE (g.is_active = true)
  GROUP BY g.id, g.group_name, g.key_version, g.created_at, g.updated_at, g.member_count, g.encrypted_group_keys;


ALTER VIEW public.group_key_audit OWNER TO vaultless;

--
-- Name: VIEW group_key_audit; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON VIEW public.group_key_audit IS 'Audit view showing group key versions and member counts for security monitoring';


--
-- Name: group_message_read_receipts; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.group_message_read_receipts (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    message_id uuid NOT NULL,
    group_id uuid NOT NULL,
    client_address uuid NOT NULL,
    read_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.group_message_read_receipts OWNER TO vaultless;

--
-- Name: TABLE group_message_read_receipts; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.group_message_read_receipts IS 'Track who read which group messages';


--
-- Name: iot_device_revocations; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.iot_device_revocations (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    application_id uuid NOT NULL,
    device_id uuid NOT NULL,
    device_cn text NOT NULL,
    device_certificate_hash text NOT NULL,
    reason text NOT NULL,
    revoked_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.iot_device_revocations OWNER TO vaultless;

--
-- Name: iot_devices; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.iot_devices (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    application_id uuid NOT NULL,
    user_id uuid NOT NULL,
    device_cn text NOT NULL,
    secure_element_id text,
    public_key_der bytea NOT NULL,
    manufacturer text,
    model text,
    hardware_revision text,
    firmware_version text,
    status public.iot_device_status DEFAULT 'active'::public.iot_device_status NOT NULL,
    registered_at timestamp with time zone DEFAULT now() NOT NULL,
    last_seen timestamp with time zone
);


ALTER TABLE public.iot_devices OWNER TO vaultless;

--
-- Name: login_attempts; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.login_attempts (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    email character varying(255) NOT NULL,
    ip_address inet NOT NULL,
    success boolean NOT NULL,
    failure_reason character varying(255),
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.login_attempts OWNER TO vaultless;

--
-- Name: TABLE login_attempts; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.login_attempts IS 'Security audit trail for login attempts';


--
-- Name: message_dlq; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.message_dlq (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    msg_id uuid NOT NULL,
    reason text NOT NULL,
    retry_count integer DEFAULT 0 NOT NULL,
    original_data text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    processed_at timestamp with time zone
);


ALTER TABLE public.message_dlq OWNER TO vaultless;

--
-- Name: message_reactions; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.message_reactions (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    message_id uuid NOT NULL,
    client_id uuid NOT NULL,
    encrypted_reaction text NOT NULL,
    nonce character varying(32) NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.message_reactions OWNER TO vaultless;

--
-- Name: TABLE message_reactions; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.message_reactions IS 'Encrypted reactions to messages. Reactions are encrypted with the same key as the message.';


--
-- Name: messages; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.messages (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    ciphertext text NOT NULL,
    nonce uuid NOT NULL,
    content_type character varying(100) DEFAULT 'application/octet-stream'::character varying,
    content_size_bytes bigint NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    accessed_at timestamp with time zone,
    access_count bigint DEFAULT 0 NOT NULL,
    is_delivered boolean DEFAULT false NOT NULL,
    delivered_at timestamp with time zone,
    max_access_count bigint,
    require_proof_verification boolean DEFAULT false NOT NULL,
    sender_client_id uuid,
    recipient_client_id uuid,
    group_id uuid,
    is_group_message boolean DEFAULT false NOT NULL,
    application_id uuid NOT NULL,
    encryption_algorithm character varying(32) DEFAULT 'xchacha20-poly1305'::character varying,
    algorithm_version smallint DEFAULT 1,
    CONSTRAINT chk_group_message_consistency CHECK ((((is_group_message = false) AND (group_id IS NULL) AND (recipient_client_id IS NOT NULL)) OR ((is_group_message = true) AND (group_id IS NOT NULL) AND (recipient_client_id IS NULL)))),
    CONSTRAINT valid_access_count CHECK ((access_count >= 0)),
    CONSTRAINT valid_content_size CHECK ((content_size_bytes > 0)),
    CONSTRAINT valid_max_access CHECK (((max_access_count IS NULL) OR (max_access_count > 0)))
);


ALTER TABLE public.messages OWNER TO vaultless;

--
-- Name: TABLE messages; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.messages IS 'Encrypted message storage - backend never sees plaintext';


--
-- Name: COLUMN messages.ciphertext; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.messages.ciphertext IS 'AES-256-GCM encrypted payload (base64)';


--
-- Name: COLUMN messages.nonce; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.messages.nonce IS '96-bit nonce for AES-GCM (base64)';


--
-- Name: COLUMN messages.encryption_algorithm; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.messages.encryption_algorithm IS 'Algorithm used: aes-256-gcm (legacy) or xchacha20-poly1305 (current)';


--
-- Name: COLUMN messages.algorithm_version; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.messages.algorithm_version IS 'Version number for algorithm parameters and key derivation method';


--
-- Name: CONSTRAINT chk_group_message_consistency ON messages; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON CONSTRAINT chk_group_message_consistency ON public.messages IS 'Ensures group messages have group_id and NULL recipient, while direct messages have recipient_id and NULL group_id';


--
-- Name: monthly_revenue_by_application; Type: VIEW; Schema: public; Owner: vaultless
--

CREATE VIEW public.monthly_revenue_by_application AS
 SELECT application_id,
    month,
    total_revenue_cents,
    total_messages,
    total_bytes,
    total_proofs,
    billing_records_count,
    avg_cost_per_record,
    min_cost_record,
    max_cost_record
   FROM _timescaledb_internal._materialized_hypertable_7;


ALTER VIEW public.monthly_revenue_by_application OWNER TO vaultless;

--
-- Name: monthly_revenue_by_developer; Type: VIEW; Schema: public; Owner: vaultless
--

CREATE VIEW public.monthly_revenue_by_developer AS
 SELECT developer_id,
    month,
    total_revenue_cents,
    applications_count,
    api_keys_count,
    total_messages,
    total_bytes
   FROM _timescaledb_internal._materialized_hypertable_8 _materialized_hypertable_8;


ALTER VIEW public.monthly_revenue_by_developer OWNER TO vaultless;

--
-- Name: webhooks; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.webhooks (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    application_id uuid NOT NULL,
    url text NOT NULL,
    event_type character varying(100) NOT NULL,
    signing_secret character varying(255) NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.webhooks OWNER TO vaultless;

--
-- Name: mv_applications_with_usage; Type: MATERIALIZED VIEW; Schema: public; Owner: vaultless
--

CREATE MATERIALIZED VIEW public.mv_applications_with_usage AS
 SELECT a.id AS application_id,
    a.developer_id,
    a.name,
    a.description,
    a.is_active,
    a.created_at,
    a.updated_at,
    a.app_meta,
    s.id AS subscription_id,
    s.tier,
    s.monthly_message_quota,
    s.monthly_bandwidth_quota,
    s.rate_limit_per_minute,
    s.message_retention_seconds,
    sk.id AS secret_key_id,
    sk.key_prefix AS secret_key_prefix,
    COALESCE(pk_data.count, (0)::bigint) AS publishable_key_count,
    COALESCE(pk_data.keys_json, '[]'::jsonb) AS publishable_keys,
    COALESCE(webhook_data.count, (0)::bigint) AS webhook_count,
    COALESCE(webhook_data.webhooks_json, '[]'::jsonb) AS webhooks,
    COALESCE(client_data.count, (0)::bigint) AS client_count,
    COALESCE(current_month.total_messages_sent, (0)::bigint) AS current_month_messages_sent,
    COALESCE(current_month.total_messages_received, (0)::bigint) AS current_month_messages_received,
    COALESCE(current_month.total_proofs_verified, (0)::bigint) AS current_month_proofs_verified,
    COALESCE(current_month.total_bytes_stored, (0)::bigint) AS current_month_bytes_stored,
    COALESCE(current_month.total_bytes_sent, (0)::bigint) AS current_month_bytes_sent,
    COALESCE(current_month.total_bytes_received, (0)::bigint) AS current_month_bytes_received,
    COALESCE(current_month.total_rate_limit_hits, (0)::bigint) AS current_month_rate_limit_hits,
    COALESCE(current_month.total_estimated_cost_cents, (0)::bigint) AS current_month_cost_cents,
    (COALESCE(revenue_data.current_month_revenue_cents, (0)::numeric))::bigint AS current_month_revenue_cents,
    (COALESCE(revenue_data.billable_clients_count, (0)::bigint))::integer AS billable_clients_count,
        CASE
            WHEN ((s.monthly_message_quota IS NOT NULL) AND (s.monthly_message_quota > 0)) THEN ((((COALESCE(current_month.total_messages_sent, (0)::bigint))::double precision / (s.monthly_message_quota)::double precision) * (100)::double precision))::numeric(5,2)
            ELSE (0)::numeric
        END AS quota_usage_percentage,
        CASE
            WHEN ((s.monthly_bandwidth_quota IS NOT NULL) AND (s.monthly_bandwidth_quota > 0)) THEN ((((COALESCE((current_month.total_bytes_sent + current_month.total_bytes_received), (0)::bigint))::double precision / (s.monthly_bandwidth_quota)::double precision) * (100)::double precision))::numeric(5,2)
            ELSE (0)::numeric
        END AS bandwidth_quota_usage_percentage,
    (COALESCE(lifetime.total_messages_sent, (0)::numeric))::bigint AS lifetime_messages_sent,
    (COALESCE(lifetime.total_estimated_cost_cents, (0)::numeric))::bigint AS lifetime_cost_cents
   FROM ((((((((public.applications a
     LEFT JOIN public.developer_subscriptions s ON ((a.subscription_id = s.id)))
     LEFT JOIN public.api_keys sk ON (((sk.application_id = a.id) AND (sk.key_type = 'secret'::public.key_type) AND (sk.is_active = true))))
     LEFT JOIN LATERAL ( SELECT count(pk.id) AS count,
            jsonb_agg(jsonb_build_object('id', pk.id, 'keyPrefix', pk.key_prefix, 'publishableKeyPlaintext', pk.publishable_key_plaintext, 'description', pk.description, 'isActive', pk.is_active, 'createdAt', pk.created_at, 'expiresAt', pk.expires_at, 'lastUsedAt', pk.last_used_at) ORDER BY pk.created_at DESC) AS keys_json
           FROM public.api_keys pk
          WHERE ((pk.application_id = a.id) AND (pk.key_type = 'publishable'::public.key_type) AND (pk.is_active = true))) pk_data ON (true))
     LEFT JOIN LATERAL ( SELECT count(c.id) AS count
           FROM public.clients c
          WHERE ((c.application_id = a.id) AND (c.is_active = true))) client_data ON (true))
     LEFT JOIN LATERAL ( SELECT (sum(umd.total_messages_sent))::bigint AS total_messages_sent,
            (sum(umd.total_messages_received))::bigint AS total_messages_received,
            (sum(umd.total_proofs_verified))::bigint AS total_proofs_verified,
            (sum(umd.total_bytes_stored))::bigint AS total_bytes_stored,
            (sum(umd.total_bytes_sent))::bigint AS total_bytes_sent,
            (sum(umd.total_bytes_received))::bigint AS total_bytes_received,
            (sum(umd.total_rate_limit_hits))::bigint AS total_rate_limit_hits,
            (sum(umd.total_estimated_cost_cents))::bigint AS total_estimated_cost_cents
           FROM _timescaledb_internal._materialized_hypertable_3 umd
          WHERE ((umd.application_id = a.id) AND (umd.day >= date_trunc('month'::text, now())))) current_month ON (true))
     LEFT JOIN LATERAL ( SELECT sum((COALESCE((cbu.revenue_snapshot ->> 'total_cost_cents'::text), '0'::text))::bigint) AS current_month_revenue_cents,
            count(DISTINCT cbu.client_id) AS billable_clients_count
           FROM (public.client_billing_usage cbu
             JOIN public.billing_periods bp ON ((cbu.billing_period_id = bp.id)))
          WHERE ((cbu.application_id = a.id) AND (bp.period_start >= date_trunc('month'::text, now())) AND (bp.period_end <= now()) AND (bp.status <> 'closed'::text))) revenue_data ON (true))
     LEFT JOIN LATERAL ( SELECT sum(umd.total_messages_sent) AS total_messages_sent,
            sum(umd.total_estimated_cost_cents) AS total_estimated_cost_cents
           FROM _timescaledb_internal._materialized_hypertable_3 umd
          WHERE (umd.application_id = a.id)) lifetime ON (true))
     LEFT JOIN LATERAL ( SELECT count(w.id) AS count,
            jsonb_agg(jsonb_build_object('id', w.id, 'url', w.url, 'eventType', w.event_type, 'isActive', w.is_active, 'createdAt', w.created_at, 'updatedAt', w.updated_at)) AS webhooks_json
           FROM public.webhooks w
          WHERE ((w.application_id = a.id) AND (w.is_active = true))) webhook_data ON (true))
  WITH NO DATA;


ALTER MATERIALIZED VIEW public.mv_applications_with_usage OWNER TO vaultless;

--
-- Name: notifications; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.notifications (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    user_id uuid NOT NULL,
    title text NOT NULL,
    message text NOT NULL,
    notification_type public.notification_type NOT NULL,
    severity public.notification_severity DEFAULT 'info'::public.notification_severity NOT NULL,
    action_url text,
    metadata jsonb,
    is_read boolean DEFAULT false NOT NULL,
    read_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone,
    CONSTRAINT valid_read_at CHECK (((read_at IS NULL) OR (is_read = true)))
);


ALTER TABLE public.notifications OWNER TO vaultless;

--
-- Name: TABLE notifications; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.notifications IS 'User notifications with classification and expiry';


--
-- Name: COLUMN notifications.notification_type; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.notifications.notification_type IS 'Category of notification for filtering';


--
-- Name: COLUMN notifications.severity; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.notifications.severity IS 'Priority level (info, warning, critical)';


--
-- Name: COLUMN notifications.action_url; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.notifications.action_url IS 'Deep link for user action (e.g., upgrade page)';


--
-- Name: COLUMN notifications.metadata; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.notifications.metadata IS 'Extensible JSON context (e.g., quota percentages)';


--
-- Name: COLUMN notifications.expires_at; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.notifications.expires_at IS 'Auto-delete notification after this date';


--
-- Name: notification_summary; Type: VIEW; Schema: public; Owner: vaultless
--

CREATE VIEW public.notification_summary AS
 SELECT user_id,
    notification_type,
    severity,
    count(*) AS total_count,
    count(*) FILTER (WHERE (is_read = false)) AS unread_count,
    max(created_at) AS latest_notification
   FROM public.notifications
  WHERE ((expires_at IS NULL) OR (expires_at > now()))
  GROUP BY user_id, notification_type, severity;


ALTER VIEW public.notification_summary OWNER TO vaultless;

--
-- Name: oauth_scopes; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.oauth_scopes (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    scope character varying(255) NOT NULL,
    description text NOT NULL,
    is_default boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.oauth_scopes OWNER TO vaultless;

--
-- Name: TABLE oauth_scopes; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.oauth_scopes IS 'OAuth 2.0 style permission scopes';


--
-- Name: popular_group_files; Type: VIEW; Schema: public; Owner: vaultless
--

CREATE VIEW public.popular_group_files AS
 SELECT gf.id,
    gf.group_id,
    gf.encrypted_filename,
    gf.file_size_bytes,
    gf.download_count,
    gf.created_at,
    g.group_name
   FROM (public.group_files gf
     JOIN public.message_groups g ON ((gf.group_id = g.id)))
  WHERE ((gf.expires_at IS NULL) OR (gf.expires_at > now()))
  ORDER BY gf.download_count DESC;


ALTER VIEW public.popular_group_files OWNER TO vaultless;

--
-- Name: pricing_plans; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.pricing_plans (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    developer_id uuid NOT NULL,
    name text NOT NULL,
    pricing_mode public.pricing_mode_enum NOT NULL,
    price_per_message_cents bigint,
    price_per_gb_cents bigint,
    price_per_proof_cents bigint,
    prepaid_amount_cents bigint,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT pricing_plan_valid CHECK ((((pricing_mode = 'free'::public.pricing_mode_enum) AND (price_per_message_cents IS NULL) AND (price_per_gb_cents IS NULL) AND (price_per_proof_cents IS NULL) AND (prepaid_amount_cents IS NULL)) OR ((pricing_mode = 'postpaid'::public.pricing_mode_enum) AND ((price_per_message_cents IS NOT NULL) OR (price_per_gb_cents IS NOT NULL) OR (price_per_proof_cents IS NOT NULL)) AND (prepaid_amount_cents IS NULL)) OR ((pricing_mode = 'prepaid'::public.pricing_mode_enum) AND (prepaid_amount_cents IS NOT NULL))))
);


ALTER TABLE public.pricing_plans OWNER TO vaultless;

--
-- Name: refresh_tokens; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.refresh_tokens (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    user_id uuid NOT NULL,
    session_id uuid,
    token_hash character varying(64) NOT NULL,
    token_family uuid NOT NULL,
    parent_token_id uuid,
    expires_at timestamp with time zone NOT NULL,
    is_used boolean DEFAULT false NOT NULL,
    used_at timestamp with time zone,
    is_revoked boolean DEFAULT false NOT NULL,
    revoked_at timestamp with time zone,
    revoked_reason character varying(255),
    device_id character varying(255),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT valid_expiry CHECK ((expires_at > created_at))
);


ALTER TABLE public.refresh_tokens OWNER TO vaultless;

--
-- Name: TABLE refresh_tokens; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.refresh_tokens IS 'Long-lived refresh tokens (30-90 days) with rotation';


--
-- Name: COLUMN refresh_tokens.token_family; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.refresh_tokens.token_family IS 'Detects token theft - if old token reused, revoke entire family';


--
-- Name: sender_keys; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.sender_keys (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    group_id uuid NOT NULL,
    sender_client_id uuid NOT NULL,
    recipient_client_id uuid NOT NULL,
    encrypted_chain_key text NOT NULL,
    key_version integer DEFAULT 1 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.sender_keys OWNER TO vaultless;

--
-- Name: TABLE sender_keys; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.sender_keys IS 'Stores encrypted chain keys for Sender Keys Protocol. Each sender maintains keys for all recipients.';


--
-- Name: session_keys; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.session_keys (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    client_id uuid NOT NULL,
    peer_client_id uuid NOT NULL,
    application_id uuid NOT NULL,
    session_id character varying(64) NOT NULL,
    ephemeral_public_key text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    algorithm_version smallint DEFAULT 1 NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    CONSTRAINT valid_expiry CHECK ((expires_at > created_at))
);


ALTER TABLE public.session_keys OWNER TO vaultless;

--
-- Name: TABLE session_keys; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.session_keys IS 'Ephemeral session keys for forward secrecy in client-to-client communication';


--
-- Name: usage_metrics_daily; Type: VIEW; Schema: public; Owner: vaultless
--

CREATE VIEW public.usage_metrics_daily AS
 SELECT application_id,
    subscription_id,
    day,
    total_messages_sent,
    total_messages_received,
    total_proofs_verified,
    total_bytes_stored,
    total_bytes_sent,
    total_bytes_received,
    total_rate_limit_hits,
    total_estimated_cost_cents
   FROM _timescaledb_internal._materialized_hypertable_3;


ALTER VIEW public.usage_metrics_daily OWNER TO vaultless;

--
-- Name: usage_metrics_keys_daily; Type: VIEW; Schema: public; Owner: vaultless
--

CREATE VIEW public.usage_metrics_keys_daily AS
 SELECT application_id,
    api_key_id,
    day,
    total_messages_sent,
    total_rate_limit_hits
   FROM _timescaledb_internal._materialized_hypertable_4;


ALTER VIEW public.usage_metrics_keys_daily OWNER TO vaultless;

--
-- Name: user_sessions; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.user_sessions (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    user_id uuid NOT NULL,
    access_token_hash character varying(64) NOT NULL,
    token_type character varying(50) DEFAULT 'Bearer'::character varying NOT NULL,
    scope text,
    expires_at timestamp with time zone NOT NULL,
    user_agent text,
    ip_address inet,
    device_id character varying(255),
    is_active boolean DEFAULT true NOT NULL,
    revoked_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    last_used_at timestamp with time zone,
    CONSTRAINT valid_expiry CHECK ((expires_at > created_at))
);


ALTER TABLE public.user_sessions OWNER TO vaultless;

--
-- Name: TABLE user_sessions; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.user_sessions IS 'Session audit trail - primary storage is Dragonfly/Redis';


--
-- Name: COLUMN user_sessions.access_token_hash; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.user_sessions.access_token_hash IS 'SHA-256 hash of access token for lookup';


--
-- Name: COLUMN user_sessions.scope; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON COLUMN public.user_sessions.scope IS 'OAuth scopes for this session (space-separated)';


--
-- Name: users; Type: TABLE; Schema: public; Owner: vaultless
--

CREATE TABLE public.users (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    email character varying(255) NOT NULL,
    password_hash character varying(255) NOT NULL,
    name character varying(255),
    avatar_url text,
    email_verified boolean DEFAULT false NOT NULL,
    email_verification_token character varying(255),
    email_verification_expires_at timestamp with time zone,
    password_reset_token character varying(255),
    password_reset_expires_at timestamp with time zone,
    is_active boolean DEFAULT true NOT NULL,
    is_admin boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    last_login_at timestamp with time zone,
    stripe_customer_id character varying(255),
    metadata jsonb
);


ALTER TABLE public.users OWNER TO vaultless;

--
-- Name: TABLE users; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TABLE public.users IS 'Core user identity and authentication';


--
-- Name: _sqlx_migrations _sqlx_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public._sqlx_migrations
    ADD CONSTRAINT _sqlx_migrations_pkey PRIMARY KEY (version);


--
-- Name: api_keys api_keys_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.api_keys
    ADD CONSTRAINT api_keys_pkey PRIMARY KEY (id);


--
-- Name: application_pricing_plans application_pricing_plans_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.application_pricing_plans
    ADD CONSTRAINT application_pricing_plans_pkey PRIMARY KEY (application_id, pricing_plan_id);


--
-- Name: applications applications_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.applications
    ADD CONSTRAINT applications_pkey PRIMARY KEY (id);


--
-- Name: billing_periods billing_periods_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.billing_periods
    ADD CONSTRAINT billing_periods_pkey PRIMARY KEY (id);


--
-- Name: client_billing_usage client_billing_usage_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_billing_usage
    ADD CONSTRAINT client_billing_usage_pkey PRIMARY KEY (id);


--
-- Name: client_invoices client_invoices_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_invoices
    ADD CONSTRAINT client_invoices_pkey PRIMARY KEY (id);


--
-- Name: client_subscriptions client_subscriptions_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_subscriptions
    ADD CONSTRAINT client_subscriptions_pkey PRIMARY KEY (id);


--
-- Name: client_usage_metrics client_usage_unique_period; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_usage_metrics
    ADD CONSTRAINT client_usage_unique_period UNIQUE (application_id, client_id, period_start);


--
-- Name: clients clients_client_identifier_hash_key; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.clients
    ADD CONSTRAINT clients_client_identifier_hash_key UNIQUE (client_identifier_hash);


--
-- Name: clients clients_identifier_key; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.clients
    ADD CONSTRAINT clients_identifier_key UNIQUE (identifier);


--
-- Name: clients clients_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.clients
    ADD CONSTRAINT clients_pkey PRIMARY KEY (id);


--
-- Name: clients clients_signing_key_key; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.clients
    ADD CONSTRAINT clients_signing_key_key UNIQUE (signing_key);


--
-- Name: file_chunks file_chunks_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.file_chunks
    ADD CONSTRAINT file_chunks_pkey PRIMARY KEY (id);


--
-- Name: file_chunks file_chunks_unique; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.file_chunks
    ADD CONSTRAINT file_chunks_unique UNIQUE (file_id, chunk_index);


--
-- Name: group_files group_files_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.group_files
    ADD CONSTRAINT group_files_pkey PRIMARY KEY (id);


--
-- Name: group_members group_members_group_id_client_address_key; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.group_members
    ADD CONSTRAINT group_members_group_id_client_address_key UNIQUE (group_id, client_address);


--
-- Name: group_members group_members_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.group_members
    ADD CONSTRAINT group_members_pkey PRIMARY KEY (id);


--
-- Name: group_message_read_receipts group_message_read_receipts_message_id_client_address_key; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.group_message_read_receipts
    ADD CONSTRAINT group_message_read_receipts_message_id_client_address_key UNIQUE (message_id, client_address);


--
-- Name: group_message_read_receipts group_message_read_receipts_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.group_message_read_receipts
    ADD CONSTRAINT group_message_read_receipts_pkey PRIMARY KEY (id);


--
-- Name: iot_device_revocations iot_device_revocations_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.iot_device_revocations
    ADD CONSTRAINT iot_device_revocations_pkey PRIMARY KEY (id);


--
-- Name: iot_devices iot_devices_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.iot_devices
    ADD CONSTRAINT iot_devices_pkey PRIMARY KEY (id);


--
-- Name: login_attempts login_attempts_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.login_attempts
    ADD CONSTRAINT login_attempts_pkey PRIMARY KEY (id);


--
-- Name: message_dlq message_dlq_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.message_dlq
    ADD CONSTRAINT message_dlq_pkey PRIMARY KEY (id);


--
-- Name: message_groups message_groups_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.message_groups
    ADD CONSTRAINT message_groups_pkey PRIMARY KEY (id);


--
-- Name: message_reactions message_reactions_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.message_reactions
    ADD CONSTRAINT message_reactions_pkey PRIMARY KEY (id);


--
-- Name: message_reactions message_reactions_unique; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.message_reactions
    ADD CONSTRAINT message_reactions_unique UNIQUE (message_id, client_id, encrypted_reaction);


--
-- Name: messages messages_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_pkey PRIMARY KEY (id);


--
-- Name: notifications notifications_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_pkey PRIMARY KEY (id);


--
-- Name: oauth_scopes oauth_scopes_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.oauth_scopes
    ADD CONSTRAINT oauth_scopes_pkey PRIMARY KEY (id);


--
-- Name: oauth_scopes oauth_scopes_scope_key; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.oauth_scopes
    ADD CONSTRAINT oauth_scopes_scope_key UNIQUE (scope);


--
-- Name: pricing_plans pricing_plans_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.pricing_plans
    ADD CONSTRAINT pricing_plans_pkey PRIMARY KEY (id);


--
-- Name: refresh_tokens refresh_tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_pkey PRIMARY KEY (id);


--
-- Name: refresh_tokens refresh_tokens_token_hash_key; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_token_hash_key UNIQUE (token_hash);


--
-- Name: sender_keys sender_keys_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.sender_keys
    ADD CONSTRAINT sender_keys_pkey PRIMARY KEY (id);


--
-- Name: sender_keys sender_keys_unique; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.sender_keys
    ADD CONSTRAINT sender_keys_unique UNIQUE (group_id, sender_client_id, recipient_client_id);


--
-- Name: session_keys session_keys_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.session_keys
    ADD CONSTRAINT session_keys_pkey PRIMARY KEY (id);


--
-- Name: developer_subscriptions subscriptions_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.developer_subscriptions
    ADD CONSTRAINT subscriptions_pkey PRIMARY KEY (id);


--
-- Name: billing_periods unique_billing_period; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.billing_periods
    ADD CONSTRAINT unique_billing_period UNIQUE (application_id, period_start);


--
-- Name: client_invoices unique_client_invoice_period; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_invoices
    ADD CONSTRAINT unique_client_invoice_period UNIQUE (client_id, billing_period_id);


--
-- Name: iot_devices unique_device_per_app; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.iot_devices
    ADD CONSTRAINT unique_device_per_app UNIQUE (application_id, device_cn);


--
-- Name: iot_device_revocations unique_revocation_per_cert; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.iot_device_revocations
    ADD CONSTRAINT unique_revocation_per_cert UNIQUE (application_id, device_certificate_hash);


--
-- Name: iot_devices unique_secure_element_per_app; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.iot_devices
    ADD CONSTRAINT unique_secure_element_per_app UNIQUE (application_id, secure_element_id);


--
-- Name: user_sessions user_sessions_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.user_sessions
    ADD CONSTRAINT user_sessions_pkey PRIMARY KEY (id);


--
-- Name: users users_email_key; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_email_key UNIQUE (email);


--
-- Name: users users_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);


--
-- Name: webhooks webhooks_app_url_type_unique; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.webhooks
    ADD CONSTRAINT webhooks_app_url_type_unique UNIQUE (application_id, url, event_type);


--
-- Name: webhooks webhooks_pkey; Type: CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.webhooks
    ADD CONSTRAINT webhooks_pkey PRIMARY KEY (id);


--
-- Name: _materialized_hypertable_3_application_id_day_idx; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX _materialized_hypertable_3_application_id_day_idx ON _timescaledb_internal._materialized_hypertable_3 USING btree (application_id, day DESC);


--
-- Name: _materialized_hypertable_3_day_idx; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX _materialized_hypertable_3_day_idx ON _timescaledb_internal._materialized_hypertable_3 USING btree (day DESC);


--
-- Name: _materialized_hypertable_3_subscription_id_day_idx; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX _materialized_hypertable_3_subscription_id_day_idx ON _timescaledb_internal._materialized_hypertable_3 USING btree (subscription_id, day DESC);


--
-- Name: _materialized_hypertable_4_api_key_id_day_idx; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX _materialized_hypertable_4_api_key_id_day_idx ON _timescaledb_internal._materialized_hypertable_4 USING btree (api_key_id, day DESC);


--
-- Name: _materialized_hypertable_4_application_id_day_idx; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX _materialized_hypertable_4_application_id_day_idx ON _timescaledb_internal._materialized_hypertable_4 USING btree (application_id, day DESC);


--
-- Name: _materialized_hypertable_4_day_idx; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX _materialized_hypertable_4_day_idx ON _timescaledb_internal._materialized_hypertable_4 USING btree (day DESC);


--
-- Name: _materialized_hypertable_6_application_id_period_start_idx; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX _materialized_hypertable_6_application_id_period_start_idx ON _timescaledb_internal._materialized_hypertable_6 USING btree (application_id, period_start DESC);


--
-- Name: _materialized_hypertable_6_client_id_period_start_idx; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX _materialized_hypertable_6_client_id_period_start_idx ON _timescaledb_internal._materialized_hypertable_6 USING btree (client_id, period_start DESC);


--
-- Name: _materialized_hypertable_6_period_start_idx; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX _materialized_hypertable_6_period_start_idx ON _timescaledb_internal._materialized_hypertable_6 USING btree (period_start DESC);


--
-- Name: _materialized_hypertable_7_application_id_month_idx; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX _materialized_hypertable_7_application_id_month_idx ON _timescaledb_internal._materialized_hypertable_7 USING btree (application_id, month DESC);


--
-- Name: _materialized_hypertable_7_month_idx; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX _materialized_hypertable_7_month_idx ON _timescaledb_internal._materialized_hypertable_7 USING btree (month DESC);


--
-- Name: _materialized_hypertable_8_developer_id_month_idx; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX _materialized_hypertable_8_developer_id_month_idx ON _timescaledb_internal._materialized_hypertable_8 USING btree (developer_id, month DESC);


--
-- Name: _materialized_hypertable_8_month_idx; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX _materialized_hypertable_8_month_idx ON _timescaledb_internal._materialized_hypertable_8 USING btree (month DESC);


--
-- Name: idx_monthly_revenue_developer_month; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX idx_monthly_revenue_developer_month ON _timescaledb_internal._materialized_hypertable_8 USING btree (developer_id, month);


--
-- Name: idx_monthly_revenue_month_application; Type: INDEX; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE INDEX idx_monthly_revenue_month_application ON _timescaledb_internal._materialized_hypertable_7 USING btree (month, application_id);


--
-- Name: api_keys_key_hash_key; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE UNIQUE INDEX api_keys_key_hash_key ON public.api_keys USING btree (key_hash) WHERE (key_hash IS NOT NULL);


--
-- Name: client_usage_metrics_period_start_idx; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX client_usage_metrics_period_start_idx ON public.client_usage_metrics USING btree (period_start DESC);


--
-- Name: idx_api_keys_active; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_api_keys_active ON public.api_keys USING btree (is_active) WHERE (is_active = true);


--
-- Name: idx_api_keys_application_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_api_keys_application_id ON public.api_keys USING btree (application_id);


--
-- Name: idx_api_keys_key_prefix; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_api_keys_key_prefix ON public.api_keys USING btree (key_prefix);


--
-- Name: idx_api_keys_one_secret_per_app; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE UNIQUE INDEX idx_api_keys_one_secret_per_app ON public.api_keys USING btree (application_id) WHERE ((key_type = 'secret'::public.key_type) AND (is_active = true));


--
-- Name: idx_api_keys_publishable_key_plaintext; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE UNIQUE INDEX idx_api_keys_publishable_key_plaintext ON public.api_keys USING btree (publishable_key_plaintext) WHERE (key_type = 'publishable'::public.key_type);


--
-- Name: idx_api_keys_user_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_api_keys_user_id ON public.api_keys USING btree (user_id);


--
-- Name: idx_applications_active; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_applications_active ON public.applications USING btree (is_active) WHERE (is_active = true);


--
-- Name: idx_applications_app_meta_gin; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_applications_app_meta_gin ON public.applications USING gin (app_meta);


--
-- Name: idx_applications_deletion_requested; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_applications_deletion_requested ON public.applications USING btree (deletion_requested_at) WHERE (deletion_requested_at IS NOT NULL);


--
-- Name: idx_applications_developer_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_applications_developer_id ON public.applications USING btree (developer_id);


--
-- Name: idx_applications_rotation_check; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_applications_rotation_check ON public.applications USING btree (is_key_rotation_forced, updated_at) WHERE (is_key_rotation_forced = true);


--
-- Name: idx_applications_subscription_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_applications_subscription_id ON public.applications USING btree (subscription_id);


--
-- Name: idx_chunks_file; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_chunks_file ON public.file_chunks USING btree (file_id, chunk_index);


--
-- Name: idx_client_usage_application_period; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_client_usage_application_period ON public.client_usage_metrics USING btree (application_id, period_start DESC);


--
-- Name: idx_client_usage_client_period; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_client_usage_client_period ON public.client_usage_metrics USING btree (client_id, period_start DESC);


--
-- Name: idx_clients_active; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_clients_active ON public.clients USING btree (is_active) WHERE (is_active = true);


--
-- Name: idx_clients_active_dev; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_clients_active_dev ON public.clients USING btree (developer_id) WHERE (is_active = true);


--
-- Name: idx_clients_app_active; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_clients_app_active ON public.clients USING btree (application_id, is_active) WHERE ((application_id IS NOT NULL) AND (is_active = true));


--
-- Name: idx_clients_application_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_clients_application_id ON public.clients USING btree (application_id);


--
-- Name: idx_clients_attested; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_clients_attested ON public.clients USING btree (application_id, is_platform_attested) WHERE (is_platform_attested = true);


--
-- Name: idx_clients_identifier; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_clients_identifier ON public.clients USING btree (identifier);


--
-- Name: idx_clients_identifier_hash; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_clients_identifier_hash ON public.clients USING btree (client_identifier_hash);


--
-- Name: idx_clients_last_message; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_clients_last_message ON public.clients USING btree (last_message_at DESC NULLS LAST);


--
-- Name: idx_clients_last_seen; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_clients_last_seen ON public.clients USING btree (last_seen_at DESC NULLS LAST);


--
-- Name: idx_clients_signing_key; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_clients_signing_key ON public.clients USING btree (signing_key) WHERE (signing_key IS NOT NULL);


--
-- Name: idx_developer_subscriptions_user_active; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_developer_subscriptions_user_active ON public.developer_subscriptions USING btree (developer_id) WHERE (is_active = true);


--
-- Name: idx_dlq_created; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_dlq_created ON public.message_dlq USING btree (created_at);


--
-- Name: idx_dlq_msg_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_dlq_msg_id ON public.message_dlq USING btree (msg_id);


--
-- Name: idx_dlq_unprocessed; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_dlq_unprocessed ON public.message_dlq USING btree (created_at) WHERE (processed_at IS NULL);


--
-- Name: idx_files_downloads; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_files_downloads ON public.group_files USING btree (download_count DESC, created_at DESC);


--
-- Name: idx_files_expires; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_files_expires ON public.group_files USING btree (expires_at) WHERE (expires_at IS NOT NULL);


--
-- Name: idx_files_group; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_files_group ON public.group_files USING btree (group_id, created_at DESC);


--
-- Name: idx_files_message; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_files_message ON public.group_files USING btree (message_id) WHERE (message_id IS NOT NULL);


--
-- Name: idx_files_uploader; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_files_uploader ON public.group_files USING btree (uploader_client_id, created_at DESC);


--
-- Name: idx_group_members_active; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_group_members_active ON public.group_members USING btree (group_id, status) WHERE (status = 'active'::public.member_status_enum);


--
-- Name: idx_group_members_client; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_group_members_client ON public.group_members USING btree (client_address);


--
-- Name: idx_group_members_group; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_group_members_group ON public.group_members USING btree (group_id, status);


--
-- Name: idx_group_members_sender_key; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_group_members_sender_key ON public.group_members USING btree (group_id, client_address, sender_key_version) WHERE (sender_chain_public_key IS NOT NULL);


--
-- Name: idx_groups_active; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_groups_active ON public.message_groups USING btree (is_active) WHERE (is_active = true);


--
-- Name: idx_groups_creator; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_groups_creator ON public.message_groups USING btree (creator_client_address);


--
-- Name: idx_groups_key_version; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_groups_key_version ON public.message_groups USING btree (key_version) WHERE (is_active = true);


--
-- Name: idx_groups_key_version_updated; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_groups_key_version_updated ON public.message_groups USING btree (key_version, updated_at DESC) WHERE (is_active = true);


--
-- Name: idx_groups_last_message; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_groups_last_message ON public.message_groups USING btree (last_message_at DESC);


--
-- Name: idx_iot_devices_app_status; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_iot_devices_app_status ON public.iot_devices USING btree (application_id, status);


--
-- Name: idx_iot_devices_last_seen; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_iot_devices_last_seen ON public.iot_devices USING btree (last_seen) WHERE (last_seen IS NOT NULL);


--
-- Name: idx_iot_devices_secure_element_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_iot_devices_secure_element_id ON public.iot_devices USING btree (secure_element_id) WHERE (secure_element_id IS NOT NULL);


--
-- Name: idx_iot_devices_user_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_iot_devices_user_id ON public.iot_devices USING btree (user_id);


--
-- Name: idx_iot_revocation_app_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_iot_revocation_app_id ON public.iot_device_revocations USING btree (application_id);


--
-- Name: idx_iot_revocation_cert_hash; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_iot_revocation_cert_hash ON public.iot_device_revocations USING btree (device_certificate_hash);


--
-- Name: idx_iot_revocation_device_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_iot_revocation_device_id ON public.iot_device_revocations USING btree (device_id);


--
-- Name: idx_login_attempts_cleanup; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_login_attempts_cleanup ON public.login_attempts USING btree (created_at);


--
-- Name: idx_login_attempts_email; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_login_attempts_email ON public.login_attempts USING btree (email, created_at DESC);


--
-- Name: idx_login_attempts_ip; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_login_attempts_ip ON public.login_attempts USING btree (ip_address, created_at DESC);


--
-- Name: idx_messages_algorithm; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_messages_algorithm ON public.messages USING btree (encryption_algorithm, algorithm_version);


--
-- Name: idx_messages_application_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_messages_application_id ON public.messages USING btree (application_id);


--
-- Name: idx_messages_conversation; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_messages_conversation ON public.messages USING btree (sender_client_id, recipient_client_id, created_at DESC);


--
-- Name: idx_messages_conversation_lookup; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_messages_conversation_lookup ON public.messages USING btree (sender_client_id, recipient_client_id, created_at DESC) WHERE ((sender_client_id IS NOT NULL) AND (recipient_client_id IS NOT NULL));


--
-- Name: idx_messages_created; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_messages_created ON public.messages USING btree (created_at DESC);


--
-- Name: idx_messages_delivered; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_messages_delivered ON public.messages USING btree (is_delivered) WHERE (is_delivered = false);


--
-- Name: idx_messages_delivered_at; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_messages_delivered_at ON public.messages USING btree (delivered_at) WHERE (delivered_at IS NOT NULL);


--
-- Name: idx_messages_expires; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_messages_expires ON public.messages USING btree (expires_at);


--
-- Name: idx_messages_group; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_messages_group ON public.messages USING btree (group_id, created_at DESC) WHERE (group_id IS NOT NULL);


--
-- Name: INDEX idx_messages_group; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON INDEX public.idx_messages_group IS 'Optimizes group message retrieval. Critical for E2EE group messaging performance.';


--
-- Name: idx_messages_recipient_client; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_messages_recipient_client ON public.messages USING btree (recipient_client_id, created_at DESC);


--
-- Name: idx_messages_recipient_undelivered; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_messages_recipient_undelivered ON public.messages USING btree (recipient_client_id, is_delivered, created_at) WHERE (is_delivered = false);


--
-- Name: idx_messages_sender_client; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_messages_sender_client ON public.messages USING btree (sender_client_id, created_at DESC);


--
-- Name: idx_mv_app_usage_app_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE UNIQUE INDEX idx_mv_app_usage_app_id ON public.mv_applications_with_usage USING btree (application_id);


--
-- Name: idx_mv_app_usage_bandwidth_warning; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_mv_app_usage_bandwidth_warning ON public.mv_applications_with_usage USING btree (developer_id, bandwidth_quota_usage_percentage DESC) WHERE (bandwidth_quota_usage_percentage >= (80)::numeric);


--
-- Name: idx_mv_app_usage_client_count; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_mv_app_usage_client_count ON public.mv_applications_with_usage USING btree (developer_id, client_count DESC);


--
-- Name: idx_mv_app_usage_developer_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_mv_app_usage_developer_id ON public.mv_applications_with_usage USING btree (developer_id);


--
-- Name: idx_mv_app_usage_quota_warning; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_mv_app_usage_quota_warning ON public.mv_applications_with_usage USING btree (developer_id, quota_usage_percentage DESC) WHERE (quota_usage_percentage >= (80)::numeric);


--
-- Name: idx_mv_app_usage_revenue_warning; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_mv_app_usage_revenue_warning ON public.mv_applications_with_usage USING btree (developer_id, current_month_revenue_cents DESC) WHERE (current_month_revenue_cents > 0);


--
-- Name: idx_notifications_expires_at; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_notifications_expires_at ON public.notifications USING btree (expires_at) WHERE (expires_at IS NOT NULL);


--
-- Name: idx_notifications_severity; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_notifications_severity ON public.notifications USING btree (severity);


--
-- Name: idx_notifications_type; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_notifications_type ON public.notifications USING btree (notification_type);


--
-- Name: idx_notifications_user_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_notifications_user_id ON public.notifications USING btree (user_id);


--
-- Name: idx_notifications_user_id_created_at; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_notifications_user_id_created_at ON public.notifications USING btree (user_id, created_at DESC);


--
-- Name: idx_notifications_user_id_is_read; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_notifications_user_id_is_read ON public.notifications USING btree (user_id, is_read);


--
-- Name: idx_notifications_user_unread; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_notifications_user_unread ON public.notifications USING btree (user_id, created_at DESC) WHERE (is_read = false);


--
-- Name: idx_one_default_pricing_plan_per_app; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE UNIQUE INDEX idx_one_default_pricing_plan_per_app ON public.application_pricing_plans USING btree (application_id) WHERE (is_default = true);


--
-- Name: idx_reactions_aggregation; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_reactions_aggregation ON public.message_reactions USING btree (message_id, encrypted_reaction);


--
-- Name: idx_reactions_client; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_reactions_client ON public.message_reactions USING btree (client_id);


--
-- Name: idx_reactions_created; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_reactions_created ON public.message_reactions USING btree (created_at DESC);


--
-- Name: idx_reactions_message; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_reactions_message ON public.message_reactions USING btree (message_id);


--
-- Name: idx_read_receipts_group_client; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_read_receipts_group_client ON public.group_message_read_receipts USING btree (group_id, client_address);


--
-- Name: idx_read_receipts_message; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_read_receipts_message ON public.group_message_read_receipts USING btree (message_id);


--
-- Name: idx_refresh_tokens_active; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_refresh_tokens_active ON public.refresh_tokens USING btree (is_used, is_revoked, expires_at) WHERE ((is_used = false) AND (is_revoked = false));


--
-- Name: idx_refresh_tokens_family; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_refresh_tokens_family ON public.refresh_tokens USING btree (token_family);


--
-- Name: idx_refresh_tokens_hash; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_refresh_tokens_hash ON public.refresh_tokens USING btree (token_hash);


--
-- Name: idx_refresh_tokens_user_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_refresh_tokens_user_id ON public.refresh_tokens USING btree (user_id);


--
-- Name: idx_sender_keys_lookup; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_sender_keys_lookup ON public.sender_keys USING btree (group_id, recipient_client_id, sender_client_id, key_version DESC);


--
-- Name: idx_sender_keys_recipient; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_sender_keys_recipient ON public.sender_keys USING btree (recipient_client_id, group_id);


--
-- Name: idx_sender_keys_sender; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_sender_keys_sender ON public.sender_keys USING btree (sender_client_id, group_id);


--
-- Name: idx_sender_keys_version; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_sender_keys_version ON public.sender_keys USING btree (key_version);


--
-- Name: idx_session_keys_application; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_session_keys_application ON public.session_keys USING btree (application_id, is_active) WHERE (is_active = true);


--
-- Name: idx_session_keys_client; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_session_keys_client ON public.session_keys USING btree (client_id, is_active) WHERE (is_active = true);


--
-- Name: idx_session_keys_expires; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_session_keys_expires ON public.session_keys USING btree (expires_at) WHERE (is_active = true);


--
-- Name: idx_session_keys_peer; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_session_keys_peer ON public.session_keys USING btree (peer_client_id, is_active) WHERE (is_active = true);


--
-- Name: idx_session_keys_session_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_session_keys_session_id ON public.session_keys USING btree (session_id);


--
-- Name: idx_sessions_access_token_hash; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_sessions_access_token_hash ON public.user_sessions USING btree (access_token_hash);


--
-- Name: idx_sessions_active; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_sessions_active ON public.user_sessions USING btree (is_active, expires_at) WHERE (is_active = true);


--
-- Name: idx_sessions_expires; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_sessions_expires ON public.user_sessions USING btree (expires_at);


--
-- Name: idx_sessions_user_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_sessions_user_id ON public.user_sessions USING btree (user_id);


--
-- Name: idx_unique_active_client_app; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE UNIQUE INDEX idx_unique_active_client_app ON public.client_subscriptions USING btree (client_id, application_id) WHERE (status = 'active'::public.subscription_status_enum);


--
-- Name: idx_usage_api_key_lookup; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_usage_api_key_lookup ON public.usage_metrics USING btree (api_key_id, period_start DESC);


--
-- Name: idx_usage_app_period; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE UNIQUE INDEX idx_usage_app_period ON public.usage_metrics USING btree (application_id, subscription_id, period_start) WHERE (api_key_id IS NULL);


--
-- Name: idx_usage_application_lookup; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_usage_application_lookup ON public.usage_metrics USING btree (application_id, period_start DESC);


--
-- Name: idx_usage_developer_subscription_lookup; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_usage_developer_subscription_lookup ON public.usage_metrics USING btree (subscription_id, period_start DESC);


--
-- Name: idx_usage_unique_key_period; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE UNIQUE INDEX idx_usage_unique_key_period ON public.usage_metrics USING btree (api_key_id, application_id, subscription_id, period_start) WHERE (api_key_id IS NOT NULL);


--
-- Name: idx_users_active; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_users_active ON public.users USING btree (is_active) WHERE (is_active = true);


--
-- Name: idx_users_email; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_users_email ON public.users USING btree (email);


--
-- Name: idx_users_email_verified; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_users_email_verified ON public.users USING btree (email_verified) WHERE (email_verified = true);


--
-- Name: idx_webhooks_application_id; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX idx_webhooks_application_id ON public.webhooks USING btree (application_id);


--
-- Name: session_keys_active_pair_unique; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE UNIQUE INDEX session_keys_active_pair_unique ON public.session_keys USING btree (client_id, peer_client_id) WHERE (is_active = true);


--
-- Name: usage_metrics_period_start_idx; Type: INDEX; Schema: public; Owner: vaultless
--

CREATE INDEX usage_metrics_period_start_idx ON public.usage_metrics USING btree (period_start DESC);


--
-- Name: group_activity_summary _RETURN; Type: RULE; Schema: public; Owner: vaultless
--

CREATE OR REPLACE VIEW public.group_activity_summary AS
 SELECT g.id AS group_id,
    g.group_name,
    g.member_count,
    g.message_count,
    count(DISTINCT gf.id) AS file_count,
    sum(gf.file_size_bytes) AS total_file_size_bytes,
    count(DISTINCT mr.id) AS total_reactions,
    g.last_message_at,
    g.created_at
   FROM (((public.message_groups g
     LEFT JOIN public.group_files gf ON (((g.id = gf.group_id) AND ((gf.expires_at IS NULL) OR (gf.expires_at > now())))))
     LEFT JOIN public.messages m ON ((g.id = m.group_id)))
     LEFT JOIN public.message_reactions mr ON ((m.id = mr.message_id)))
  WHERE (g.is_active = true)
  GROUP BY g.id;


--
-- Name: _compressed_hypertable_2 ts_insert_blocker; Type: TRIGGER; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE TRIGGER ts_insert_blocker BEFORE INSERT ON _timescaledb_internal._compressed_hypertable_2 FOR EACH ROW EXECUTE FUNCTION _timescaledb_functions.insert_blocker();


--
-- Name: _materialized_hypertable_3 ts_insert_blocker; Type: TRIGGER; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE TRIGGER ts_insert_blocker BEFORE INSERT ON _timescaledb_internal._materialized_hypertable_3 FOR EACH ROW EXECUTE FUNCTION _timescaledb_functions.insert_blocker();


--
-- Name: _materialized_hypertable_4 ts_insert_blocker; Type: TRIGGER; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE TRIGGER ts_insert_blocker BEFORE INSERT ON _timescaledb_internal._materialized_hypertable_4 FOR EACH ROW EXECUTE FUNCTION _timescaledb_functions.insert_blocker();


--
-- Name: _materialized_hypertable_6 ts_insert_blocker; Type: TRIGGER; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE TRIGGER ts_insert_blocker BEFORE INSERT ON _timescaledb_internal._materialized_hypertable_6 FOR EACH ROW EXECUTE FUNCTION _timescaledb_functions.insert_blocker();


--
-- Name: _materialized_hypertable_7 ts_insert_blocker; Type: TRIGGER; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE TRIGGER ts_insert_blocker BEFORE INSERT ON _timescaledb_internal._materialized_hypertable_7 FOR EACH ROW EXECUTE FUNCTION _timescaledb_functions.insert_blocker();


--
-- Name: _materialized_hypertable_8 ts_insert_blocker; Type: TRIGGER; Schema: _timescaledb_internal; Owner: vaultless
--

CREATE TRIGGER ts_insert_blocker BEFORE INSERT ON _timescaledb_internal._materialized_hypertable_8 FOR EACH ROW EXECUTE FUNCTION _timescaledb_functions.insert_blocker();


--
-- Name: applications trigger_applications_updated_at; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_applications_updated_at BEFORE UPDATE ON public.applications FOR EACH ROW EXECUTE FUNCTION public.update_updated_at();


--
-- Name: messages trigger_cleanup_reactions; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_cleanup_reactions BEFORE DELETE ON public.messages FOR EACH ROW EXECUTE FUNCTION public.cleanup_reactions_on_message_delete();


--
-- Name: group_members trigger_cleanup_sender_keys; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_cleanup_sender_keys AFTER UPDATE ON public.group_members FOR EACH ROW WHEN ((new.status IS DISTINCT FROM old.status)) EXECUTE FUNCTION public.cleanup_sender_keys_on_member_leave();


--
-- Name: clients trigger_clients_updated_at; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_clients_updated_at BEFORE UPDATE ON public.clients FOR EACH ROW EXECUTE FUNCTION public.update_clients_updated_at();


--
-- Name: notifications trigger_notifications_updated_at; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_notifications_updated_at BEFORE UPDATE ON public.notifications FOR EACH ROW EXECUTE FUNCTION public.update_notifications_updated_at();


--
-- Name: notifications trigger_set_notification_read_at; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_set_notification_read_at BEFORE UPDATE ON public.notifications FOR EACH ROW EXECUTE FUNCTION public.set_notification_read_at();


--
-- Name: group_members trigger_suggest_key_rotation; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_suggest_key_rotation AFTER UPDATE ON public.group_members FOR EACH ROW WHEN ((new.status IS DISTINCT FROM old.status)) EXECUTE FUNCTION public.check_group_key_rotation();


--
-- Name: TRIGGER trigger_suggest_key_rotation ON group_members; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TRIGGER trigger_suggest_key_rotation ON public.group_members IS 'Logs a notification when a member status changes to suggest key rotation for security';


--
-- Name: messages trigger_update_application_last_message; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_update_application_last_message AFTER INSERT ON public.messages FOR EACH ROW EXECUTE FUNCTION public.update_application_last_message();


--
-- Name: messages trigger_update_client_last_message; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_update_client_last_message AFTER INSERT ON public.messages FOR EACH ROW EXECUTE FUNCTION public.update_client_last_message();


--
-- Name: group_members trigger_update_group_member_count; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_update_group_member_count AFTER INSERT OR UPDATE ON public.group_members FOR EACH ROW EXECUTE FUNCTION public.update_group_member_count();


--
-- Name: messages trigger_update_group_message_stats; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_update_group_message_stats AFTER INSERT ON public.messages FOR EACH ROW WHEN ((new.group_id IS NOT NULL)) EXECUTE FUNCTION public.update_group_message_stats();


--
-- Name: users trigger_users_updated_at; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_users_updated_at BEFORE UPDATE ON public.users FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();


--
-- Name: message_groups trigger_validate_encrypted_keys; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_validate_encrypted_keys BEFORE INSERT OR UPDATE ON public.message_groups FOR EACH ROW WHEN ((new.encrypted_group_keys IS NOT NULL)) EXECUTE FUNCTION public.validate_encrypted_keys_structure();


--
-- Name: TRIGGER trigger_validate_encrypted_keys ON message_groups; Type: COMMENT; Schema: public; Owner: vaultless
--

COMMENT ON TRIGGER trigger_validate_encrypted_keys ON public.message_groups IS 'Validates the structure of encrypted_group_keys JSON to prevent malformed data';


--
-- Name: webhooks trigger_webhooks_updated_at; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER trigger_webhooks_updated_at BEFORE UPDATE ON public.webhooks FOR EACH ROW EXECUTE FUNCTION public.update_updated_at();


--
-- Name: client_usage_metrics ts_cagg_invalidation_trigger; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER ts_cagg_invalidation_trigger AFTER INSERT OR DELETE OR UPDATE ON public.client_usage_metrics FOR EACH ROW EXECUTE FUNCTION _timescaledb_functions.continuous_agg_invalidation_trigger('5');


--
-- Name: usage_metrics ts_cagg_invalidation_trigger; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER ts_cagg_invalidation_trigger AFTER INSERT OR DELETE OR UPDATE ON public.usage_metrics FOR EACH ROW EXECUTE FUNCTION _timescaledb_functions.continuous_agg_invalidation_trigger('1');


--
-- Name: client_usage_metrics ts_insert_blocker; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER ts_insert_blocker BEFORE INSERT ON public.client_usage_metrics FOR EACH ROW EXECUTE FUNCTION _timescaledb_functions.insert_blocker();


--
-- Name: usage_metrics ts_insert_blocker; Type: TRIGGER; Schema: public; Owner: vaultless
--

CREATE TRIGGER ts_insert_blocker BEFORE INSERT ON public.usage_metrics FOR EACH ROW EXECUTE FUNCTION _timescaledb_functions.insert_blocker();


--
-- Name: api_keys api_keys_application_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.api_keys
    ADD CONSTRAINT api_keys_application_id_fkey FOREIGN KEY (application_id) REFERENCES public.applications(id) ON DELETE CASCADE;


--
-- Name: api_keys api_keys_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.api_keys
    ADD CONSTRAINT api_keys_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: application_pricing_plans app_pricing_plans_application_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.application_pricing_plans
    ADD CONSTRAINT app_pricing_plans_application_fkey FOREIGN KEY (application_id) REFERENCES public.applications(id) ON DELETE CASCADE;


--
-- Name: application_pricing_plans app_pricing_plans_plan_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.application_pricing_plans
    ADD CONSTRAINT app_pricing_plans_plan_fkey FOREIGN KEY (pricing_plan_id) REFERENCES public.pricing_plans(id) ON DELETE CASCADE;


--
-- Name: applications applications_developer_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.applications
    ADD CONSTRAINT applications_developer_id_fkey FOREIGN KEY (developer_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: applications applications_subscription_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.applications
    ADD CONSTRAINT applications_subscription_id_fkey FOREIGN KEY (subscription_id) REFERENCES public.developer_subscriptions(id);


--
-- Name: billing_periods billing_period_app_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.billing_periods
    ADD CONSTRAINT billing_period_app_fkey FOREIGN KEY (application_id) REFERENCES public.applications(id) ON DELETE CASCADE;


--
-- Name: client_billing_usage billing_usage_client_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_billing_usage
    ADD CONSTRAINT billing_usage_client_fkey FOREIGN KEY (client_id) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: client_billing_usage billing_usage_developer_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_billing_usage
    ADD CONSTRAINT billing_usage_developer_fkey FOREIGN KEY (developer_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: client_billing_usage billing_usage_period_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_billing_usage
    ADD CONSTRAINT billing_usage_period_fkey FOREIGN KEY (billing_period_id) REFERENCES public.billing_periods(id) ON DELETE CASCADE;


--
-- Name: client_invoices client_invoices_developer_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_invoices
    ADD CONSTRAINT client_invoices_developer_fkey FOREIGN KEY (developer_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: client_subscriptions client_subscriptions_application_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_subscriptions
    ADD CONSTRAINT client_subscriptions_application_fkey FOREIGN KEY (application_id) REFERENCES public.applications(id) ON DELETE CASCADE;


--
-- Name: client_subscriptions client_subscriptions_client_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_subscriptions
    ADD CONSTRAINT client_subscriptions_client_fkey FOREIGN KEY (client_id) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: client_subscriptions client_subscriptions_plan_app_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_subscriptions
    ADD CONSTRAINT client_subscriptions_plan_app_fkey FOREIGN KEY (application_id, pricing_plan_id) REFERENCES public.application_pricing_plans(application_id, pricing_plan_id);


--
-- Name: client_usage_metrics client_usage_application_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_usage_metrics
    ADD CONSTRAINT client_usage_application_fkey FOREIGN KEY (application_id) REFERENCES public.applications(id) ON DELETE CASCADE;


--
-- Name: client_usage_metrics client_usage_client_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_usage_metrics
    ADD CONSTRAINT client_usage_client_fkey FOREIGN KEY (client_id) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: clients clients_application_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.clients
    ADD CONSTRAINT clients_application_id_fkey FOREIGN KEY (application_id) REFERENCES public.applications(id) ON DELETE CASCADE;


--
-- Name: clients clients_developer_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.clients
    ADD CONSTRAINT clients_developer_id_fkey FOREIGN KEY (developer_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: file_chunks file_chunks_file_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.file_chunks
    ADD CONSTRAINT file_chunks_file_fkey FOREIGN KEY (file_id) REFERENCES public.group_files(id) ON DELETE CASCADE;


--
-- Name: group_files group_files_group_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.group_files
    ADD CONSTRAINT group_files_group_fkey FOREIGN KEY (group_id) REFERENCES public.message_groups(id) ON DELETE CASCADE;


--
-- Name: group_files group_files_message_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.group_files
    ADD CONSTRAINT group_files_message_fkey FOREIGN KEY (message_id) REFERENCES public.messages(id) ON DELETE CASCADE;


--
-- Name: group_files group_files_uploader_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.group_files
    ADD CONSTRAINT group_files_uploader_fkey FOREIGN KEY (uploader_client_id) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: group_members group_members_client_address_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.group_members
    ADD CONSTRAINT group_members_client_address_fkey FOREIGN KEY (client_address) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: group_members group_members_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.group_members
    ADD CONSTRAINT group_members_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.message_groups(id) ON DELETE CASCADE;


--
-- Name: group_message_read_receipts group_message_read_receipts_client_address_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.group_message_read_receipts
    ADD CONSTRAINT group_message_read_receipts_client_address_fkey FOREIGN KEY (client_address) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: group_message_read_receipts group_message_read_receipts_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.group_message_read_receipts
    ADD CONSTRAINT group_message_read_receipts_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.message_groups(id) ON DELETE CASCADE;


--
-- Name: group_message_read_receipts group_message_read_receipts_message_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.group_message_read_receipts
    ADD CONSTRAINT group_message_read_receipts_message_id_fkey FOREIGN KEY (message_id) REFERENCES public.messages(id) ON DELETE CASCADE;


--
-- Name: client_invoices invoice_period_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.client_invoices
    ADD CONSTRAINT invoice_period_fkey FOREIGN KEY (billing_period_id) REFERENCES public.billing_periods(id) ON DELETE CASCADE;


--
-- Name: iot_device_revocations iot_device_revocations_application_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.iot_device_revocations
    ADD CONSTRAINT iot_device_revocations_application_id_fkey FOREIGN KEY (application_id) REFERENCES public.applications(id) ON DELETE CASCADE;


--
-- Name: iot_device_revocations iot_device_revocations_device_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.iot_device_revocations
    ADD CONSTRAINT iot_device_revocations_device_id_fkey FOREIGN KEY (device_id) REFERENCES public.iot_devices(id) ON DELETE CASCADE;


--
-- Name: iot_devices iot_devices_application_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.iot_devices
    ADD CONSTRAINT iot_devices_application_id_fkey FOREIGN KEY (application_id) REFERENCES public.applications(id) ON DELETE CASCADE;


--
-- Name: iot_devices iot_devices_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.iot_devices
    ADD CONSTRAINT iot_devices_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: message_groups message_groups_creator_client_address_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.message_groups
    ADD CONSTRAINT message_groups_creator_client_address_fkey FOREIGN KEY (creator_client_address) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: message_reactions message_reactions_client_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.message_reactions
    ADD CONSTRAINT message_reactions_client_fkey FOREIGN KEY (client_id) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: message_reactions message_reactions_message_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.message_reactions
    ADD CONSTRAINT message_reactions_message_fkey FOREIGN KEY (message_id) REFERENCES public.messages(id) ON DELETE CASCADE;


--
-- Name: messages messages_application_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_application_id_fkey FOREIGN KEY (application_id) REFERENCES public.applications(id) ON DELETE CASCADE;


--
-- Name: messages messages_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.message_groups(id) ON DELETE CASCADE;


--
-- Name: messages messages_recipient_client_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_recipient_client_id_fkey FOREIGN KEY (recipient_client_id) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: messages messages_sender_client_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_sender_client_id_fkey FOREIGN KEY (sender_client_id) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: notifications notifications_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: pricing_plans pricing_plans_developer_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.pricing_plans
    ADD CONSTRAINT pricing_plans_developer_fkey FOREIGN KEY (developer_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: refresh_tokens refresh_tokens_parent_token_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_parent_token_id_fkey FOREIGN KEY (parent_token_id) REFERENCES public.refresh_tokens(id);


--
-- Name: refresh_tokens refresh_tokens_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.user_sessions(id) ON DELETE CASCADE;


--
-- Name: refresh_tokens refresh_tokens_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: sender_keys sender_keys_group_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.sender_keys
    ADD CONSTRAINT sender_keys_group_fkey FOREIGN KEY (group_id) REFERENCES public.message_groups(id) ON DELETE CASCADE;


--
-- Name: sender_keys sender_keys_recipient_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.sender_keys
    ADD CONSTRAINT sender_keys_recipient_fkey FOREIGN KEY (recipient_client_id) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: sender_keys sender_keys_sender_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.sender_keys
    ADD CONSTRAINT sender_keys_sender_fkey FOREIGN KEY (sender_client_id) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: session_keys session_keys_application_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.session_keys
    ADD CONSTRAINT session_keys_application_id_fkey FOREIGN KEY (application_id) REFERENCES public.applications(id) ON DELETE CASCADE;


--
-- Name: session_keys session_keys_client_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.session_keys
    ADD CONSTRAINT session_keys_client_id_fkey FOREIGN KEY (client_id) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: session_keys session_keys_peer_client_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.session_keys
    ADD CONSTRAINT session_keys_peer_client_id_fkey FOREIGN KEY (peer_client_id) REFERENCES public.clients(id) ON DELETE CASCADE;


--
-- Name: developer_subscriptions subscriptions_developer_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.developer_subscriptions
    ADD CONSTRAINT subscriptions_developer_id_fkey FOREIGN KEY (developer_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: usage_metrics usage_metrics_api_key_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.usage_metrics
    ADD CONSTRAINT usage_metrics_api_key_id_fkey FOREIGN KEY (api_key_id) REFERENCES public.api_keys(id) ON DELETE SET NULL;


--
-- Name: usage_metrics usage_metrics_application_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.usage_metrics
    ADD CONSTRAINT usage_metrics_application_id_fkey FOREIGN KEY (application_id) REFERENCES public.applications(id) ON DELETE CASCADE;


--
-- Name: usage_metrics usage_metrics_subscription_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.usage_metrics
    ADD CONSTRAINT usage_metrics_subscription_id_fkey FOREIGN KEY (subscription_id) REFERENCES public.developer_subscriptions(id) ON DELETE CASCADE;


--
-- Name: user_sessions user_sessions_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.user_sessions
    ADD CONSTRAINT user_sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: webhooks webhooks_application_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: vaultless
--

ALTER TABLE ONLY public.webhooks
    ADD CONSTRAINT webhooks_application_id_fkey FOREIGN KEY (application_id) REFERENCES public.applications(id) ON DELETE CASCADE;


--
-- PostgreSQL database dump complete
--

