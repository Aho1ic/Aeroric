import { beforeEach, describe, expect, it } from "vitest";
import {
  countTerminalCreated,
  countTerminalDisposed,
  countTimerCleared,
  countTimerRegistered,
  registerProseCacheProbe,
  registerWebglContextProbe,
  resetResourceCensus,
  resourceCensusSnapshot,
} from "../lib/resourceCensus";

/**
 * 存活普查的语义:登记「当前还活着几个」,不是累计总量。
 *
 * 这个仪表的价值全在「持平 vs 只涨不跌」的区分上 —— 如果它自己记成累计,那读出来永远在涨,
 * 就分不出真泄漏了。所以这里主要盯:减法真的减、探针不被自己记一份。
 */

beforeEach(() => {
  resetResourceCensus();
  // 探针是模块级注入的,真实模块 import 时已经注册过。测试里覆盖成可控的。
  registerWebglContextProbe(() => 0);
  registerProseCacheProbe(
    () => 0,
    () => 0,
  );
});

describe("resourceCensus", () => {
  it("初始快照全为零", () => {
    expect(resourceCensusSnapshot()).toEqual({
      liveTerminals: 0,
      liveWebglContexts: 0,
      proseCacheEntries: 0,
      proseCacheChars: 0,
      liveTimers: 0,
    });
  });

  it("终端创建后计数上涨", () => {
    countTerminalCreated();
    countTerminalCreated();
    expect(resourceCensusSnapshot().liveTerminals).toBe(2);
  });

  // 这条是整个仪表的意义所在:dispose 必须真的减回去,否则读数永远在涨,分不出真泄漏。
  it("终端销毁后计数减回去", () => {
    countTerminalCreated();
    countTerminalCreated();
    countTerminalDisposed();
    expect(resourceCensusSnapshot().liveTerminals).toBe(1);
    countTerminalDisposed();
    expect(resourceCensusSnapshot().liveTerminals).toBe(0);
  });

  it("timer 注册与注销对称", () => {
    countTimerRegistered();
    countTimerRegistered();
    countTimerRegistered();
    expect(resourceCensusSnapshot().liveTimers).toBe(3);
    countTimerCleared();
    countTimerCleared();
    countTimerCleared();
    expect(resourceCensusSnapshot().liveTimers).toBe(0);
  });

  it("开关一轮后回到原点(空转应当持平)", () => {
    const before = resourceCensusSnapshot();
    countTerminalCreated();
    countTimerRegistered();
    countTerminalDisposed();
    countTimerCleared();
    expect(resourceCensusSnapshot()).toEqual(before);
  });

  it("WebGL 计数走注入的探针,不自己记一份", () => {
    let contexts = 0;
    registerWebglContextProbe(() => contexts);
    expect(resourceCensusSnapshot().liveWebglContexts).toBe(0);
    contexts = 4;
    expect(resourceCensusSnapshot().liveWebglContexts).toBe(4);
    contexts = 1;
    expect(resourceCensusSnapshot().liveWebglContexts).toBe(1);
  });

  it("prose cache 的条数与字符数各走自己的探针", () => {
    registerProseCacheProbe(
      () => 42,
      () => 123_456,
    );
    const snapshot = resourceCensusSnapshot();
    expect(snapshot.proseCacheEntries).toBe(42);
    expect(snapshot.proseCacheChars).toBe(123_456);
  });

  it("reset 只清自持计数,不动探针", () => {
    registerWebglContextProbe(() => 7);
    countTerminalCreated();
    resetResourceCensus();
    const snapshot = resourceCensusSnapshot();
    expect(snapshot.liveTerminals).toBe(0);
    // 探针还在:它读的是别的模块的真实状态,不该被普查的 reset 抹掉。
    expect(snapshot.liveWebglContexts).toBe(7);
  });

  // 快照是读时求值,不是订阅式缓存 —— 每次读都要拿到最新的数。
  it("快照每次读都重新求值", () => {
    let contexts = 0;
    registerWebglContextProbe(() => contexts);
    const first = resourceCensusSnapshot();
    contexts = 3;
    const second = resourceCensusSnapshot();
    expect(first.liveWebglContexts).toBe(0);
    expect(second.liveWebglContexts).toBe(3);
  });
});
