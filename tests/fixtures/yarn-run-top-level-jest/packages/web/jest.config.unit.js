module.exports = {
  setupFilesAfterEnv: ['<rootDir>/tests/setup-unit.js'],
  moduleNameMapper: {
    '^inquirer$': '<rootDir>/src/test/mocks/inquirer.js',
    '^prompts$': ['<rootDir>/src/test/mocks/missing.js', '<rootDir>/src/test/mocks/prompts'],
    '^@/(.*)$': '<rootDir>/src/$1',
    '\\.css$': 'identity-obj-proxy',
  },
};
