export const fmt = (n: number) => new Intl.NumberFormat('en-US', {maximumFractionDigits: 0}).format(n);
export const units = (n: number) => n >= 1e6 ? (n/1e6).toLocaleString('en-US', {maximumFractionDigits:2})+'M' : n >= 1000 ? (n/1000).toLocaleString('en-US',{maximumFractionDigits:1})+'K' : fmt(n);
export const money = (n: number) => new Intl.NumberFormat('en-US',{style:'currency',currency:'USD',minimumFractionDigits:2,maximumFractionDigits:2}).format(n);
export const precise = (n: number) => n.toLocaleString('en-US',{minimumFractionDigits:2,maximumFractionDigits:4});
