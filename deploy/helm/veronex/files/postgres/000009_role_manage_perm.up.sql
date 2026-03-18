-- 000009: Add role_manage permission to super role
UPDATE roles SET permissions = array_append(permissions, 'role_manage') WHERE name = 'super' AND NOT ('role_manage' = ANY(permissions));
