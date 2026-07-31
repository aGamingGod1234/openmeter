import type { Snapshot } from "./models";

export interface ProviderLayout {
  metricOrder: string[];
  onDemand: string[];
  hidden: string[];
  starred: string[];
  expanded: boolean;
}

export interface Layout {
  providerOrder: string[];
  providers: Record<string, ProviderLayout>;
}

export function snapshotCardId(snapshot: Snapshot): string {
  return snapshot.card_id?.trim() || snapshot.id;
}

export function emptyProviderLayout(snapshot?: Snapshot): ProviderLayout {
  const metricOrder = [...new Set((snapshot?.metrics ?? []).map((metric) => metric.label))];
  let onDemand = (snapshot?.metrics ?? [])
    .filter((metric) => metric.kind !== "progress")
    .map((metric) => metric.label);
  onDemand = [...new Set(onDemand)];
  if (metricOrder.length > 0 && metricOrder.length === onDemand.length) onDemand = [];
  return { metricOrder, onDemand, hidden: [], starred: [], expanded: false };
}

export function migrateLayout(
  existing: Layout | null,
  snapshots: Snapshot[],
  createProviderLayout: (snapshot: Snapshot) => ProviderLayout = emptyProviderLayout,
): Layout {
  const layout: Layout = existing
    ? {
        providerOrder: [...existing.providerOrder],
        providers: Object.fromEntries(
          Object.entries(existing.providers).map(([id, provider]) => [
            id,
            {
              metricOrder: [...provider.metricOrder],
              onDemand: [...provider.onDemand],
              hidden: [...provider.hidden],
              starred: [...provider.starred],
              expanded: provider.expanded,
            },
          ]),
        ),
      }
    : { providerOrder: [], providers: {} };

  for (const snapshot of snapshots) {
    const cardId = snapshotCardId(snapshot);
    if (!layout.providerOrder.includes(cardId)) layout.providerOrder.push(cardId);
    if (!layout.providers[cardId]) layout.providers[cardId] = createProviderLayout(snapshot);
  }

  return layout;
}
