export function purchaseQuantityError(quantity:string,remaining:string):string {
 if(!/^[1-9][0-9]{0,8}$/.test(quantity))return 'Enter a positive whole number of credits.';
 if(BigInt(quantity)>BigInt(remaining))return `Only ${BigInt(remaining).toLocaleString('en-US')} credits remain. Reduce the purchase quantity.`;
 return '';
}
