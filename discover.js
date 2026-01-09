// aave-api-explorer.js
const AAVE_API = 'https://api.v3.aave.com/graphql';

async function queryAave(query, variables = {}) {
  const response = await fetch(AAVE_API, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query, variables }),
  });
  
  const result = await response.json();
  if (result.errors) {
    console.error('Errors:', JSON.stringify(result.errors, null, 2));
  }
  return result.data;
}

async function main() {
  // Get ALL query fields
  console.log('=== ALL AVAILABLE QUERIES ===\n');
  
  const schemaQuery = `
    {
      __schema {
        queryType {
          fields {
            name
            description
            args {
              name
              type {
                name
                kind
                ofType { name kind }
              }
            }
          }
        }
      }
    }
  `;
  
  const schema = await queryAave(schemaQuery);
  const fields = schema.__schema.queryType.fields;
  
  // Look for user/position/liquidation related queries
  const keywords = ['user', 'position', 'liquidat', 'account', 'health', 'market', 'reserve'];
  
  for (const field of fields) {
    const isRelevant = keywords.some(kw => 
      field.name.toLowerCase().includes(kw) || 
      (field.description && field.description.toLowerCase().includes(kw))
    );
    
    if (isRelevant || fields.indexOf(field) < 50) {
      const args = field.args.map(a => {
        const typeName = a.type.name || a.type.ofType?.name || a.type.kind;
        return `${a.name}: ${typeName}`;
      }).join(', ');
      console.log(`${field.name}(${args})`);
      if (field.description) {
        console.log(`  └─ ${field.description.slice(0, 100)}`);
      }
    }
  }

  // Introspect specific types
  console.log('\n\n=== INTROSPECTING KEY TYPES ===\n');
  
  const types = ['Market', 'User', 'Position', 'UserPosition', 'Reserve', 'Account'];
  
  for (const typeName of types) {
    const typeQuery = `
      {
        __type(name: "${typeName}") {
          name
          kind
          fields {
            name
            type { name kind ofType { name } }
          }
          inputFields {
            name
            type { name kind ofType { name } }
          }
        }
      }
    `;
    
    const typeData = await queryAave(typeQuery);
    if (typeData?.__type) {
      console.log(`\n--- ${typeName} ---`);
      const t = typeData.__type;
      if (t.fields) {
        for (const f of t.fields.slice(0, 15)) {
          const ft = f.type.name || f.type.ofType?.name || f.type.kind;
          console.log(`  ${f.name}: ${ft}`);
        }
        if (t.fields.length > 15) console.log(`  ... and ${t.fields.length - 15} more fields`);
      }
      if (t.inputFields) {
        for (const f of t.inputFields) {
          const ft = f.type.name || f.type.ofType?.name || f.type.kind;
          console.log(`  ${f.name}: ${ft}`);
        }
      }
    }
  }

  // Try to get markets with proper request
  console.log('\n\n=== TRYING MARKETS QUERY ===\n');
  
  // First introspect the request type
  const marketRequestQuery = `
    {
      __type(name: "MarketsRequest") {
        inputFields {
          name
          type { name kind ofType { name kind } }
        }
      }
    }
  `;
  
  const marketReqType = await queryAave(marketRequestQuery);
  if (marketReqType?.__type) {
    console.log('MarketsRequest fields:');
    for (const f of marketReqType.__type.inputFields || []) {
      console.log(`  ${f.name}: ${f.type.name || f.type.ofType?.name || f.type.kind}`);
    }
  }

  // Introspect UserPositionsRequest
  console.log('\n\n=== USER POSITIONS REQUEST ===\n');
  const userPosQuery = `
    {
      __type(name: "UserPositionsRequest") {
        inputFields {
          name
          type { name kind ofType { name kind } }
        }
      }
    }
  `;
  
  const userPosType = await queryAave(userPosQuery);
  if (userPosType?.__type) {
    console.log('UserPositionsRequest fields:');
    for (const f of userPosType.__type.inputFields || []) {
      console.log(`  ${f.name}: ${f.type.name || f.type.ofType?.name || f.type.kind}`);
    }
  }
}

main().catch(console.error);
