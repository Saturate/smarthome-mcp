export interface HAEntityState {
  entity_id: string;
  state: string;
  attributes: Record<string, unknown>;
  last_changed: string;
  last_updated: string;
  context: {
    id: string;
    parent_id: string | null;
    user_id: string | null;
  };
}

export interface HAServiceResponse {
  entity_id?: string;
  state?: string;
  attributes?: Record<string, unknown>;
}

export interface HATemplateResult {
  result: string;
}

export interface HAConfig {
  url: string;
  token: string;
}
