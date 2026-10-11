import * as ns from './c';
class Sub extends ns.C {}
Sub.live();
const held=new Sub();
async function make():Promise<Sub>{return (()=>held)();}
async function read():Promise<void>{const item=await make();item.asyncUsed();const promise=make();promise.promiseWrong();}
read();
function syncMake():Sub{return new Sub();}
const {destructuredUsed}=syncMake();destructuredUsed();
let item=new Sub();const saved=item;item.receiverUsed();
({item}={item:{receiverWrong(){}}});item.receiverWrong();saved.receiverRetained();
let factory=()=>new Sub();const factorySaved=factory();
({factory}={factory:()=>({factoryWrong(){}})});const changed=factory();changed.factoryWrong();factorySaved.factorySaved();
const holder={item:new Sub()};const child=holder.item;holder.item.containerUsed();
({item:holder.item}={item:{containerWrong(){}}});holder.item.containerWrong();child.containerRetained();
let iteration=new Sub();const iterationSaved=iteration;iteration.iterationUsed();
for(iteration of [{iterationWrong(){}}]){iteration.iterationWrong();}iterationSaved.iterationRetained();
async function all():Promise<void>{const values=await Promise.all([0].map(async():Promise<Sub>=>new Sub()));for(const value of values)value.unsupportedPromiseAll();}
all();
class GetterHolder{get item():Sub{return new Sub();}}
const getter=new GetterHolder();getter.item.getterUsed();
class ConstructorHolder{constructor(public item:Sub){}}
const constructor=new ConstructorHolder(new Sub());constructor.item.constructorUsed();
class ShadowGetter<Sub>{get item():Sub{return null as Sub;}}
new ShadowGetter<{getterWrong():void}>().item.getterWrong();
class ShadowConstructor<Sub>{constructor(public item:Sub){}}
const shadowConstructor=new ShadowConstructor({constructorWrong(){}});shadowConstructor.item.constructorWrong();
const nestedHolder={item:new Sub()};const nestedSaved=nestedHolder.item;nestedHolder.item.nestedUsed();
({nested:[nestedHolder.item]}={nested:[{nestedWrong(){}}]});nestedHolder.item.nestedWrong();nestedSaved.nestedRetained();
let defaultItem=new Sub();const defaultSaved=defaultItem;defaultItem.defaultUsed();
[defaultItem={defaultWrong(){}}]=[];defaultItem.defaultWrong();defaultSaved.defaultRetained();

class TypedThisHolder{item:Sub=new Sub();call(){const item=this.item;item.typedThisUsed();}}
new TypedThisHolder().call();
class InitializedThisHolder{item=new Sub();call(){const item=this.item;item.initializedThisUsed();}}
new InitializedThisHolder().call();
class ArrowThisHolder{item:Sub=new Sub();call(){const read=()=>{const item=this.item;item.arrowThisUsed();};read();function independent(this:{item:{independentThisWrong():void}}){const item=this.item;item.independentThisWrong();}independent.call({item:{independentThisWrong(){}}});}}
new ArrowThisHolder().call();

import { Handler } from './handler';
export class ParameterHandlerHolder {
  readonly handler?:Handler;
  constructor({Current}:{Current?:typeof Handler}){if(Current)this.handler=new Current();}
  call(){this.handler?.optionalCurrentUsed?.();this.handler!.normalCurrentUsed();}
}
new ParameterHandlerHolder({Current:Handler}).call();
