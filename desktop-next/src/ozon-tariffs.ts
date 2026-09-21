import tariffData from "./data/ozon-logistics-tariffs-2026-08-28.json";

export type OzonLogisticsTariff = {
  maxLiters: number;
  under300: number;
  over300: number;
};

type OzonLogisticsTariffBundle = {
  effectiveFrom: string;
  sourceUrl: string;
  sourceFile: string;
  origin: string;
  routes: Record<string, OzonLogisticsTariff[]>;
  fallback: OzonLogisticsTariff[];
};

const bundle = tariffData as OzonLogisticsTariffBundle;

const normalize = (value: string) => value
  .toLocaleLowerCase("ru-RU")
  .replace(/ё/g, "е")
  .replace(/[^a-zа-я0-9]+/gi, " ")
  .trim();

const destinationAliases: Record<string, string> = {
  "Москва и МО": "Москва, МО и Дальние регионы",
  "Москва, МО": "Москва, МО и Дальние регионы",
  "Санкт-Петербург": "Санкт-Петербург и СЗО",
  "Ростов-на-Дону": "Ростов",
  "Нижний Новгород": "Нижний Новгород",
};

function matchDestination(clusterName: string): string | null {
  const wanted = normalize(destinationAliases[clusterName] ?? clusterName);
  const exact = Object.keys(bundle.routes).find((name) => normalize(name) === wanted);
  if (exact) return exact;
  return Object.keys(bundle.routes).find((name) => {
    const candidate = normalize(name);
    return wanted.includes(candidate) || candidate.includes(wanted);
  }) ?? null;
}

function matchVolume(rows: OzonLogisticsTariff[], volumeLiters: number): OzonLogisticsTariff | null {
  if (!(volumeLiters > 0) || !rows.length) return null;
  return rows.find((row) => volumeLiters <= row.maxLiters + 1e-9) ?? rows.at(-1) ?? null;
}

export function officialLogisticsTariff(clusterName: string, volumeLiters: number) {
  const destination = matchDestination(clusterName);
  const route = destination ? matchVolume(bundle.routes[destination], volumeLiters) : null;
  const fallback = route ? null : matchVolume(bundle.fallback, volumeLiters);
  const tariff = route ?? fallback;
  return tariff ? {
    ...tariff,
    destination,
    isFallback: !route,
    effectiveFrom: bundle.effectiveFrom,
    sourceUrl: bundle.sourceUrl,
    origin: bundle.origin,
  } : null;
}

export const OZON_LOGISTICS_TARIFF_META = {
  effectiveFrom: bundle.effectiveFrom,
  sourceUrl: bundle.sourceUrl,
  origin: bundle.origin,
};

export const OZON_LOGISTICS_DESTINATIONS = Object.keys(bundle.routes);

export type CrossdockHandoff = "warehouse" | "pvz" | "ppz" | "courier";

/**
 * Ozon 2026-09 rules: the handling part is route-independent.  The line-haul
 * part is fixed by Ozon when the supply application is created.  1.5 RUB/L is
 * the official universal route tariff only when either tariff zone is unknown.
 */
export function officialCrossdockEstimate(totalLiters: number, handoff: CrossdockHandoff = "warehouse") {
  const billedLiters = totalLiters > 0 ? Math.ceil(totalLiters) : 0;
  const handlingPerLiter = handoff === "courier" ? 1.55 : handoff === "pvz" ? 5 : handoff === "ppz" ? 4 : 0;
  const universalRoutePerLiter = 1.5;
  return {
    billedLiters,
    handlingPerLiter,
    handling: billedLiters * handlingPerLiter,
    routePerLiter: universalRoutePerLiter,
    route: billedLiters * universalRoutePerLiter,
    total: billedLiters * (handlingPerLiter + universalRoutePerLiter),
    isUniversalFallback: true,
  };
}
