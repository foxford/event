-- Add migration script here
ALTER TABLE room_ban
ADD COLUMN IF NOT EXISTS reason TEXT;
