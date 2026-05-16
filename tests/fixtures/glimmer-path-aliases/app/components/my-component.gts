import MyService from 'my-app/services/my-service';

const Wrapper = <template>
  <div class="wrapper">{{yield}}</div>
</template>;

export default class MyComponent {
  service = MyService;
}

<template>
  <Wrapper>{{this.service}}</Wrapper>
</template>
