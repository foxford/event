-- Add migration script here

ALTER TABLE room ADD COLUMN locked_types2 jsonb NOT NULL DEFAULT '{}'::jsonb;

UPDATE room
SET locked_types2 = subq.locked_types2
FROM (
    SELECT id, jsonb_object_agg(ty, true) AS locked_types2
    FROM (
        SELECT id, UNNEST(locked_types) as ty FROM room
    ) AS subsubq GROUP BY id
) AS subq
WHERE room.id = subq.id;

ALTER TABLE room DROP COLUMN locked_types;

ALTER TABLE room RENAME COLUMN locked_types2 TO locked_types;
