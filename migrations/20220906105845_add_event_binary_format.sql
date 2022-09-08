-- add column for event binary repr
ALTER TABLE event ADD COLUMN binary_data bytea;
-- allow data to be null w/o default but only
-- iff binary_data is present and vice versa
ALTER TABLE event ALTER COLUMN data DROP NOT NULL;
ALTER TABLE event ALTER COLUMN data DROP DEFAULT;
ALTER TABLE event ADD CONSTRAINT data_or_binary_data_not_null
    CHECK ((binary_data IS NOT NULL) OR (data IS NOT NULL)) NOT VALID;
-- check data size for both data fields
ALTER TABLE event DROP CONSTRAINT data_size;
ALTER TABLE event ADD CONSTRAINT data_size
    CHECK (deleted_at IS NOT NULL
    OR (COALESCE(octet_length(data::text), 0) + COALESCE(octet_length(binary_data::bytea), 0)) < 102400) NOT VALID;
