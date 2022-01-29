create index if not exists idx_cidr_min on statement(cidr_min);
create index if not exists idx_cidr_max on statement(cidr_max);
/* some more indexes for other operations */
create index if not exists idx_entity_1 on statement(entity_1);
create index if not exists idx_opinion_fk_statement on opinion(statement_id);

