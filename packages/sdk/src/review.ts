export type ListingApplication = {
  id: number;
  applicant: string;
  family: number;
  title: string;
  tags: string[];
  event: string;
  description: string;
  topic?: string;
  tag?: string;
  blocked_regions: string[];
  dup_key: string;
  status: number;
  status_name: string;
  reviewer: string;
  reason: string;
  created_at: number;
  reviewed_at: number;
  compose?: Record<string, unknown>;
  market?: string;
  logs?: { id: number; action: string; reviewer: string; reason: string; created_at: number }[];
};

export type ApplicationPage = {
  page: number;
  limit: number;
  total: number;
  pages: number;
  items: ListingApplication[];
};

export async function submitListingApplication(
  api: string,
  body: {
    applicant: string;
    family: number;
    title: string;
    tags: string[];
    event: string;
    description: string;
    topic?: string;
    tag?: string;
    blocked_regions?: string[];
    compose?: Record<string, unknown>;
  },
): Promise<ListingApplication> {
  const r = await fetch(`${api}/v1/listings/applications`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const v = (await r.json()) as ListingApplication;
  if (r.status === 409) {
    const err = new Error("duplicate listing");
    (err as Error & { application?: ListingApplication }).application = v;
    throw err;
  }
  if (r.status === 400) throw new Error("application rejected");
  if (!r.ok) throw new Error(`application ${r.status}`);
  return v;
}

export async function listListingApplications(
  api: string,
  q: { status?: number; applicant?: string; page?: number } = {},
): Promise<ApplicationPage> {
  const p = new URLSearchParams();
  if (q.status != null) p.set("status", String(q.status));
  if (q.applicant) p.set("applicant", q.applicant);
  if (q.page) p.set("page", String(q.page));
  const r = await fetch(`${api}/v1/listings/applications?${p}`);
  if (!r.ok) throw new Error(`applications ${r.status}`);
  return r.json();
}

export async function fetchListingApplication(api: string, id: number): Promise<ListingApplication> {
  const r = await fetch(`${api}/v1/listings/applications/${id}`);
  if (r.status === 404) throw new Error("application not found");
  if (!r.ok) throw new Error(`application ${r.status}`);
  return r.json();
}

export async function reviewListingApplication(
  api: string,
  body: {
    id: number;
    reviewer: string;
    action: "approve" | "reject" | "duplicate" | "opened";
    reason?: string;
    market?: string;
  },
): Promise<ListingApplication> {
  const r = await fetch(`${api}/v1/review`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (r.status === 403) throw new Error("not a reviewer");
  if (r.status === 409) throw new Error("already reviewed");
  if (r.status === 400) throw new Error("review rejected");
  if (!r.ok) throw new Error(`review ${r.status}`);
  return r.json();
}
