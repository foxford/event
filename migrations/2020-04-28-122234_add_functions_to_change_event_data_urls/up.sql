-- Your SQL goes here
CREATE OR REPLACE FUNCTION update_event_data_urls(room_id UUID)
RETURNS INTEGER LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE
   ct int;
BEGIN
    UPDATE event
    SET
        data = CASE event.kind
                WHEN 'document' THEN
                    jsonb_set(
                        jsonb_set(event.data, '{premigration_url}', event.data->'url'),
                        '{url}',
                        storage_url
                    )
                WHEN 'draw' THEN
                    jsonb_set(
                        jsonb_set(event.data, '{premigration_src}', event.data->'src'),
                        '{src}',
                        storage_src
                    )
                ELSE data
                END
    FROM (
        SELECT id,
            to_jsonb('storage://' || reverse(split_part(reverse(data->>'url'), '/', 1))) AS storage_url,
            to_jsonb('storage://' || reverse(split_part(reverse(data->>'src'), '/', 1))) AS storage_src,
            kind
        FROM event
        WHERE (
                (kind = 'document' AND data->>'url' ILIKE '%/api/v1/%')
                OR
                (kind = 'draw' AND data->>'type' = 'image' AND data->>'src' ILIKE '%/api/v1/%')

            ) AND event.room_id = update_event_data_urls.room_id
    ) as subq
    WHERE event.id = subq.id;

    GET DIAGNOSTICS ct = ROW_COUNT;
    RETURN ct;
END;
$$;

CREATE OR REPLACE FUNCTION rollback_event_data_urls(room_id UUID)
RETURNS INTEGER LANGUAGE plpgsql SECURITY DEFINER AS $$
DECLARE
   ct int;
BEGIN
    UPDATE event
    SET
        data = CASE event.kind
                WHEN 'document' THEN jsonb_set(event.data, '{url}', old_url)
                WHEN 'draw' THEN jsonb_set(event.data, '{src}', old_src)
                ELSE data
                END
    FROM (
        SELECT id,
            data->'premigration_url' AS old_url,
            data->'premigration_src' AS old_src,
            kind
        FROM event
        WHERE (
                (kind = 'draw' AND data->>'type' = 'image' AND jsonb_path_exists(data, '$.premigration_src'))
                OR
                (kind = 'document' AND jsonb_path_exists(data, '$.premigration_url'))
            ) AND event.room_id = rollback_event_data_urls.room_id
    ) as subq
    WHERE event.id = subq.id;

    GET DIAGNOSTICS ct = ROW_COUNT;
    RETURN ct;
END;
$$;
