export * from 'a' with {
type:'json',
mode:'strict',
}
import 'b' with {
type:'json',
mode:'strict'
}
import {
  c
} from 'c' with {type:'json',mode:'strict'}
