-- remove fields which are not used
COMMIT; -- transaction must be committed for pragmas to really work
PRAGMA foreign_keys=off;
PRAGMA legacy_alter_table=on;
BEGIN TRANSACTION;

DROP INDEX idx_cidr_min;
DROP INDEX idx_cidr_max;
DROP INDEX idx_entity_1;

ALTER TABLE statement RENAME TO old_statement;

CREATE TABLE statement AS
    SELECT id, name, entity_1, entity_2, cidr_min, cidr_max, last_used, last_weight
    FROM old_statement;

create index if not exists idx_cidr_min on statement(cidr_min);
create index if not exists idx_cidr_max on statement(cidr_max);
create index if not exists idx_entity_1 on statement(entity_1);

DROP TABLE old_statement;

PRAGMA foreign_keys=on;
PRAGMA legacy_alter_table=off;
