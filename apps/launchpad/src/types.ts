import type { Project, ProjectAction } from './projects';
import type { IconName } from './components/Icon';
export type View = 'admin' | 'launchpad' | 'credits' | 'supplier' | 'launches' | 'pool' | 'prl' | 'models' | 'browse' | 'resources';
export type Family = 'Llama' | 'Qwen' | 'Mistral';
export interface Launch {
  id: string; modelId:string; name: string; supplier: string; family: Family; precision: string;
  context: string; price: number; total: number; sold: number; stake: number;
  logo: string; color: string; window: string; own: boolean; resourceId?:string;
}
export interface PoolOperator { name: string; region: string; capacity: number }
export interface Receipt {
  requestId?: string; tokens: number; model: string; id: string;
  outage: boolean; cost: number; extra: number; slash: number;
}
export interface Activity { title: string; detail: string; icon: IconName; time: string }
export interface Notice { id: string; text: string; error?: boolean }
export interface PrlTransfer {id:string;kind:'deposit'|'withdrawal';amount:number;at:string}
export interface DemoState {
  projects:Project[]; version: 2; wallet: number; credits: Record<string, number>; burned: number;
  stake: number; reserve: number; poolCapacity: number; poolOperators: PoolOperator[];
  launches: Launch[]; activity: Activity[]; lastRequest: Receipt | null;
  receipts: Receipt[]; notice: Notice | null;
  prlAvailable: number; prlTransfers: PrlTransfer[];
}
export type DemoAction = ProjectAction
  | {type:'prl-transfer';id:string;kind:'deposit'|'withdrawal';amount:number;at:string}
  | {type: 'buy'; id: string; launchId: string; quantity: number}
  | {type: 'redeem'; id: string; launchId: string; tokens: number; outage: boolean}
  | {type: 'launch'; id: string; modelId:string; name: string; family?: Family; price: number; total: number; stake: number; window: string; verified: boolean; resourceId?:string}
  | {type: 'enroll'; id: string; name: string; region: string; capacity: number}
  | {type: 'reset'; id: string};
