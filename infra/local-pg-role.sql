-- 1/3  在 pgAdmin 连到数据库 postgres，单独执行本文件。
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'cpm') THEN
        CREATE ROLE cpm LOGIN PASSWORD 'cpm';
    END IF;
END
$$;
