-- 000009: Remove role_manage permission from super role
UPDATE roles SET permissions = array_remove(permissions, 'role_manage') WHERE name = 'super';
