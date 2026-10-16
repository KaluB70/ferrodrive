import { Injectable } from '@angular/core';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';

import type { CheckResult, Scan } from './models';

/** Talks to the Rust side; serves sample data when opened in a plain browser. */
@Injectable({ providedIn: 'root' })
export class FerrodriveService {
  public scan(): Promise<Scan> {
    return isTauri() ? invoke<Scan>('scan') : this.sample<Scan>('scan');
  }

  public check(): Promise<CheckResult> {
    return isTauri() ? invoke<CheckResult>('check') : this.sample<CheckResult>('check');
  }

  public async open(url: string): Promise<void> {
    if (isTauri()) {
      await openUrl(url);
    } else {
      window.open(url, '_blank', 'noopener');
    }
  }

  private async sample<T>(name: string): Promise<T> {
    const response: Response = await fetch(`sample/${name}.json`);
    await new Promise<void>((resolve) => setTimeout(resolve, name === 'check' ? 900 : 200));
    return (await response.json()) as T;
  }
}
