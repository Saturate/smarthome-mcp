import type { HAConfig, HAEntityState } from "./types.js";

export class HAClient {
  private baseUrl: string;
  private headers: Record<string, string>;

  constructor(config: HAConfig) {
    this.baseUrl = config.url.replace(/\/$/, "");
    this.headers = {
      Authorization: `Bearer ${config.token}`,
      "Content-Type": "application/json",
    };
  }

  private async request<T>(path: string, options?: RequestInit): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      ...options,
      headers: { ...this.headers, ...options?.headers },
      signal: AbortSignal.timeout(10_000),
    });

    if (!response.ok) {
      const body = await response.text().catch(() => "");
      throw new Error(
        `HA API ${response.status}: ${response.statusText} — ${body}`,
      );
    }

    return response.json() as Promise<T>;
  }

  private async requestText(
    path: string,
    options?: RequestInit,
  ): Promise<string> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      ...options,
      headers: { ...this.headers, ...options?.headers },
      signal: AbortSignal.timeout(10_000),
    });

    if (!response.ok) {
      const body = await response.text().catch(() => "");
      throw new Error(
        `HA API ${response.status}: ${response.statusText} — ${body}`,
      );
    }

    return response.text();
  }

  async getStates(): Promise<HAEntityState[]> {
    return this.request<HAEntityState[]>("/api/states");
  }

  async getStatesByDomain(domain: string): Promise<HAEntityState[]> {
    const states = await this.getStates();
    return states.filter((s) => s.entity_id.startsWith(`${domain}.`));
  }

  async getState(entityId: string): Promise<HAEntityState> {
    return this.request<HAEntityState>(`/api/states/${entityId}`);
  }

  async callService(
    domain: string,
    service: string,
    data?: Record<string, unknown>,
  ): Promise<HAEntityState[]> {
    return this.request<HAEntityState[]>(`/api/services/${domain}/${service}`, {
      method: "POST",
      body: JSON.stringify(data ?? {}),
    });
  }

  /**
   * For services that return data (like todo.get_items) rather than state changes.
   */
  async callServiceWithResponse(
    domain: string,
    service: string,
    data?: Record<string, unknown>,
  ): Promise<unknown> {
    return this.request(`/api/services/${domain}/${service}?return_response`, {
      method: "POST",
      body: JSON.stringify(data ?? {}),
    });
  }

  /**
   * Renders a Jinja2 template via HA's REST API.
   * This is the only way to query areas/floors via REST (no direct endpoint).
   * Returns raw text to avoid JSON auto-parsing (HA returns Python-style lists).
   */
  async renderTemplate(template: string): Promise<string> {
    return this.requestText("/api/template", {
      method: "POST",
      body: JSON.stringify({ template }),
    });
  }

  async getAreas(): Promise<string[]> {
    const raw = await this.renderTemplate("{{ areas() | list }}");
    return this.parseJinjaList(raw);
  }

  async getAreaName(areaId: string): Promise<string> {
    return this.renderTemplate(`{{ area_name('${areaId}') }}`);
  }

  /**
   * Resolves entity_id → { area, device } for a batch of entities.
   * Checks entity area first, falls back to device area, and includes device name.
   */
  async getEntityMetaMap(
    entityIds: string[],
  ): Promise<Record<string, { area: string | null; device: string | null }>> {
    if (entityIds.length === 0) return {};
    const idList = entityIds.map((id) => `'${id}'`).join(", ");
    const template = `{%- set ns = namespace(d={}) -%}
{%- for eid in [${idList}] -%}
  {%- set ea = area_name(eid) -%}
  {%- set da = device_attr(eid, 'area_id') -%}
  {%- set dn = device_attr(eid, 'name_by_user') or device_attr(eid, 'name') -%}
  {%- set area = ea if ea else (area_name(da) if da else None) -%}
  {%- set ns.d = dict(ns.d, **{eid: {'area': area, 'device': dn}}) -%}
{%- endfor -%}
{{ ns.d | tojson }}`;
    const raw = await this.renderTemplate(template);
    try {
      return JSON.parse(raw);
    } catch {
      return {};
    }
  }

  async getEntitiesInArea(areaId: string): Promise<string[]> {
    const raw = await this.renderTemplate(
      `{{ area_entities('${areaId}') | list }}`,
    );
    return this.parseJinjaList(raw);
  }

  private parseJinjaList(raw: string): string[] {
    return parseJinjaList(raw);
  }
}

/**
 * Parses the string representation of a Python list that HA's template API returns.
 * e.g. "['kitchen', 'living_room']" -> ["kitchen", "living_room"]
 */
export function parseJinjaList(raw: string): string[] {
  const trimmed = raw.trim();
  if (trimmed === "[]") return [];

  return trimmed
    .slice(1, -1)
    .split(",")
    .map((s) => s.trim().replace(/^['"]|['"]$/g, ""))
    .filter(Boolean);
}
