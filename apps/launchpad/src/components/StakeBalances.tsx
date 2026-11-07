import type {ServerProject} from '../server-projects';
import {stakeBalances,stakeUnits} from '../launch-dashboard';
export function StakeBalances({project}:{project:ServerProject}){
 const balances=stakeBalances(project);
 return <section className="stake-balances" aria-label="Project PRL balances"><h3>PRL collateral</h3><dl>{([
  ['Locked',balances?.locked],['Free · withdrawable',balances?.free],['Returned to wallet',balances?.returned]
 ] as const).map(([label,value])=><div key={label}><dt>{label}</dt><dd>{value===undefined?'—':stakeUnits(value.toString())}</dd></div>)}</dl><p>{balances?'Current escrow records: partial release and a withdrawable balance are not supported yet. A full release or refund transfers tokens directly to your wallet.':'Balances appear after the stake is confirmed by the chain monitor.'}</p><p>Planned: release collateral as this provider successfully delivers and settles credits. Public-pool fallback and failed requests do not count toward the provider’s delivery.</p></section>;
}
