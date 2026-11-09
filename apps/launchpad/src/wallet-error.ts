export function walletError(error:unknown):string {
 const value=error&&typeof error==='object'?error as {code?:unknown;message?:unknown}:{};
 if(value.code===4001)return 'Wallet request cancelled. No new transaction was submitted by this action.';
 if(value.code===-32002)return 'A wallet request is already pending. Open your wallet extension and finish or reject that request.';
 if(typeof value.message==='string'&&value.message.trim())return value.message;
 return 'The operation failed. Check your wallet connection and try again.';
}
