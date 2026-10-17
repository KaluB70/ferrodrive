import { Component, computed, inject, OnInit, signal } from '@angular/core';

import { FerrodriveService } from './ferrodrive.service';
import type { Device, Report, Scan, SourceError, Update } from './models';

type Phase = 'idle' | 'busy' | 'done' | 'failed';

@Component({
  selector: 'app-root',
  templateUrl: './app.html',
  styleUrl: './app.css',
})
export class App implements OnInit {
  private readonly ferrodrive = inject(FerrodriveService);

  protected readonly scan = signal<Scan | null>(null);
  protected readonly scanPhase = signal<Phase>('idle');
  protected readonly report = signal<Report | null>(null);
  protected readonly checkPhase = signal<Phase>('idle');
  protected readonly error = signal<string | null>(null);
  protected readonly query = signal<string>('');
  protected readonly showAll = signal<boolean>(false);

  protected readonly devices = computed<readonly Device[]>(() => {
    const scan: Scan | null = this.scan();
    if (!scan) {
      return [];
    }
    const query: string = this.query().trim().toLowerCase();
    return scan.devices
      .filter((d: Device) => this.showAll() || d.driver !== null)
      .filter((d: Device) => !query || this.searchText(d).includes(query))
      .sort((a: Device, b: Device) => this.sortKey(a).localeCompare(this.sortKey(b)));
  });

  protected readonly updates = computed<readonly Update[]>(() => this.report()?.updates ?? []);

  protected readonly failures = computed<readonly SourceError[]>(() => this.report()?.errors ?? []);

  protected readonly updatedDevices = computed<ReadonlySet<string>>(
    () => new Set(this.updates().flatMap((u: Update) => (u.device ? [u.device] : []))),
  );

  ngOnInit(): void {
    void this.runScan();
  }

  protected async runScan(): Promise<void> {
    this.scanPhase.set('busy');
    try {
      this.scan.set(await this.ferrodrive.scan());
      this.scanPhase.set('done');
    } catch (e: unknown) {
      this.fail(this.scanPhase, e);
    }
  }

  protected async runCheck(): Promise<void> {
    this.checkPhase.set('busy');
    this.error.set(null);
    try {
      const result = await this.ferrodrive.check();
      this.scan.set(result.scan);
      this.report.set(result.report);
      this.checkPhase.set('done');
    } catch (e: unknown) {
      this.fail(this.checkPhase, e);
    }
  }

  protected deviceName(instanceId: string | null): string | null {
    if (!instanceId) {
      return null;
    }
    const device: Device | undefined = this.scan()?.devices.find((d: Device) => d.instance_id === instanceId);
    return device?.name ?? instanceId;
  }

  protected open(url: string): void {
    void this.ferrodrive.open(url);
  }

  protected onQuery(event: Event): void {
    this.query.set((event.target as HTMLInputElement).value);
  }

  protected onShowAll(event: Event): void {
    this.showAll.set((event.target as HTMLInputElement).checked);
  }

  private fail(phase: ReturnType<typeof signal<Phase>>, e: unknown): void {
    phase.set('failed');
    this.error.set(typeof e === 'string' ? e : e instanceof Error ? e.message : 'Something went wrong');
  }

  private searchText(device: Device): string {
    return [device.name, device.class, device.manufacturer, device.driver?.provider, device.driver?.version]
      .filter((part): part is string => !!part)
      .join(' ')
      .toLowerCase();
  }

  private sortKey(device: Device): string {
    return `${device.class ?? '~'} ${device.name ?? device.instance_id}`;
  }
}
