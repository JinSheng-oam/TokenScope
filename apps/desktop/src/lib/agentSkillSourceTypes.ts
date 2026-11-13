export type Source = { url: string; branch: string | null; subpath: string | null; skill: string | null };
export type Inspection = { source: Source; revision: string; candidates: { path: string; name: string; preview: string }[] };
export type UpdateInfo = { kind: string; source: string | null; status: string; detail: string };
export type SkillSummary = {
  skill_name: string;
  overview: string;
  usage_guide: string;
  raw_response: string;
  model: string;
  updated_at_ms: number;
};
