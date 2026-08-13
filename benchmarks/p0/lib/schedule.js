function alternatingOrders(arms, trials) {
  if (arms.length === 0) return Array.from({ length: trials }, () => []);
  return Array.from({ length: trials }, (_, trial) => {
    if (trial === 0) return [...arms];
    if (arms.length === 2) return trial % 2 === 0 ? [...arms] : [...arms].reverse();
    if (trial === 1) return [...arms].reverse();
    const order = [...arms];
    const offset = (Math.floor(arms.length / 2) + trial - 2) % arms.length;
    return order.slice(offset).concat(order.slice(0, offset));
  });
}

module.exports = { alternatingOrders };
