export interface AggregateModelSpend {
  model: string;
  cost: number;
  tokens: number;
}

export interface AggregateWindow {
  cost: number;
  tokens: number;
  models: AggregateModelSpend[];
}

export interface AggregateProviderSpend {
  id: string;
  name: string;
  today: AggregateWindow;
  yesterday: AggregateWindow;
  last30: AggregateWindow;
  trend: number[];
  unpriced: number;
  unpriced_models: string[];
  authoritative_tokens?: boolean;
}

export interface AggregateTrackingDay {
  day: string;
  provider_id: string;
  model: string;
  cost: number;
  tokens: number;
}

export interface AggregateTracking {
  devices: unknown[];
  days: AggregateTrackingDay[];
}

function shiftDay(day: string, delta: number): string {
  const date = new Date(`${day}T00:00:00Z`);
  date.setUTCDate(date.getUTCDate() + delta);
  return date.toISOString().slice(0, 10);
}

function providerFamily(id: string): string {
  return id.split("--", 1)[0].split("@", 1)[0];
}

export function selectAggregateSpend<T extends AggregateProviderSpend>(
  spend: T[],
  tracking: AggregateTracking | null,
  today: string,
  providerNames: ReadonlyMap<string, string> = new Map(),
): AggregateProviderSpend[] {
  if (!tracking || (tracking.devices.length === 0 && tracking.days.length === 0)) return spend;

  const yesterday = shiftDay(today, -1);
  const first30 = shiftDay(today, -29);
  const byProvider = new Map<string, AggregateTrackingDay[]>();
  for (const row of tracking.days) {
    const rows = byProvider.get(row.provider_id) ?? [];
    rows.push(row);
    byProvider.set(row.provider_id, rows);
  }
  const authoritative = spend.filter((row) => row.authoritative_tokens);
  const authoritativeFamilies = new Set(authoritative.map((row) => providerFamily(row.id)));
  const selected: AggregateProviderSpend[] = [];
  for (const [providerId, providerRows] of byProvider) {
    if (authoritativeFamilies.has(providerFamily(providerId))) continue;
    const window = (rows: AggregateTrackingDay[]): AggregateWindow => {
      const models = new Map<string, AggregateModelSpend>();
      for (const row of rows) {
        const model = models.get(row.model) ?? { model: row.model, cost: 0, tokens: 0 };
        model.cost += row.cost;
        model.tokens += row.tokens;
        models.set(row.model, model);
      }
      return {
        cost: rows.reduce((sum, row) => sum + row.cost, 0),
        tokens: rows.reduce((sum, row) => sum + row.tokens, 0),
        models: [...models.values()].sort((left, right) => right.cost - left.cost),
      };
    };
    selected.push({
      id: providerId,
      name: providerNames.get(providerId) ?? providerId,
      today: window(providerRows.filter((row) => row.day === today)),
      yesterday: window(providerRows.filter((row) => row.day === yesterday)),
      last30: window(providerRows.filter((row) => row.day >= first30 && row.day <= today)),
      trend: Array.from({ length: 30 }, (_, index) => {
        const day = shiftDay(today, index - 29);
        return providerRows
          .filter((row) => row.day === day)
          .reduce((sum, row) => sum + row.tokens, 0);
      }),
      unpriced: 0,
      unpriced_models: [],
    });
  }
  selected.push(...authoritative);
  return selected;
}
