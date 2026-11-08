import test from 'node:test';
import assert from 'node:assert/strict';
import {purchaseQuantityError} from './credit-purchase.ts';
test('purchase uses remaining inventory rather than original issuance supply',()=>{
 assert.match(purchaseQuantityError('1000000','999000'),/999,000/);
 assert.equal(purchaseQuantityError('999000','999000'),'');
 assert.equal(purchaseQuantityError('1000','999000'),'');
 assert.match(purchaseQuantityError('1','0'),/Only 0/);
});
test('purchase rejects empty, fractional, negative and exponential quantities',()=>{
 for(const value of ['','0','-1','1.5','1e3','1000000000'])assert.notEqual(purchaseQuantityError(value,'999000'),'');
});
