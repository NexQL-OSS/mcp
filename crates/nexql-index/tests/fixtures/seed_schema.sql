-- Fixed seed for nexql-index golden-file parity (Phase 3).
-- Keep schema/comments stable — golden expected/ is regenerated from this.

DROP TABLE IF EXISTS public.orders;
DROP TABLE IF EXISTS public.users;

CREATE TABLE public.users (
  id         serial PRIMARY KEY,
  email      text NOT NULL,
  name       text,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE public.orders (
  id      serial PRIMARY KEY,
  user_id integer NOT NULL REFERENCES public.users (id),
  total   numeric(12, 2) NOT NULL
);

COMMENT ON TABLE public.users IS 'Application accounts (aka customers)';
COMMENT ON COLUMN public.users.email IS 'Login address (aka username)';
COMMENT ON COLUMN public.users.name IS 'Display name';

COMMENT ON TABLE public.orders IS 'Purchase records (aka purchases)';
COMMENT ON COLUMN public.orders.user_id IS 'Owning account FK';
COMMENT ON COLUMN public.orders.total IS 'Order amount (aka amt)';
