/** Mirrors the serialized types of `ferrodrive-core`. */

export interface System {
  readonly manufacturer: string | null;
  readonly product: string | null;
  readonly version: string | null;
  readonly sku: string | null;
  readonly family: string | null;
  readonly board_manufacturer: string | null;
  readonly board_product: string | null;
}

export interface Firmware {
  readonly vendor: string | null;
  readonly version: string | null;
  readonly release_date: string | null;
}

export interface Driver {
  readonly version: string | null;
  readonly date: string | null;
  readonly provider: string | null;
  readonly package: string | null;
}

export interface Device {
  readonly instance_id: string;
  readonly name: string | null;
  readonly class: string | null;
  readonly manufacturer: string | null;
  readonly hardware_ids: readonly string[];
  readonly driver: Driver | null;
}

export interface Scan {
  readonly system: System;
  readonly firmware: Firmware;
  readonly devices: readonly Device[];
}

export type UpdateKind = 'driver' | 'firmware';

export interface Update {
  readonly source: string;
  readonly kind: UpdateKind;
  readonly title: string;
  readonly version: string | null;
  readonly date: string | null;
  readonly url: string | null;
  readonly device: string | null;
  readonly installed: string | null;
}

export interface SourceError {
  readonly source: string;
  readonly message: string;
}

export interface Report {
  readonly updates: readonly Update[];
  readonly errors: readonly SourceError[];
}

export interface CheckResult {
  readonly scan: Scan;
  readonly report: Report;
}
