-- Run before migration in all environments
-- update room set kind = '${KIND}' where kind is null;
alter table room
    alter column kind set not null;
